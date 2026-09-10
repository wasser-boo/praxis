#!/usr/bin/env python3
"""Read-only structural inspection / safe media extraction for .mnemodim v1.

Adapted from the Pi mnemodim-palace helper. Python 3.9+, standard library only.
Not the application's complete importer, media decoder or formula validator.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import csv
import io
import json
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
from typing import Any
import zipfile

TOTAL = 100 * 1024 * 1024
MEDIA = 20 * 1024 * 1024
METADATA = 700 * 1024
EXTENSIONS = {
    "image/png": ".png", "image/jpeg": ".jpg", "image/webp": ".webp",
    "image/gif": ".gif", "image/avif": ".avif", "audio/mpeg": ".mp3",
    "audio/wav": ".wav", "audio/x-wav": ".wav", "audio/ogg": ".ogg",
    "audio/webm": ".webm", "audio/mp4": ".m4a",
}


def require(test: bool, message: str) -> None:
    if not test:
        raise ValueError(message)


def identifier(value: Any) -> str:
    require(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,128}", value) is not None,
            "Invalid package ID (expected 1..128 letters/digits/_/-)")
    return value


def safe_path(name: str) -> None:
    path = PurePosixPath(name)
    require(bool(name) and len(name) <= 512 and not path.is_absolute()
            and "\\" not in name and not any(ord(c) < 32 for c in name)
            and all(p not in ("", ".", "..") for p in name.rstrip("/").split("/")),
            "Unsafe ZIP member path")


def load_json(text: str) -> Any:
    def invalid_constant(value):
        raise ValueError("Non-finite JSON number")
    return json.loads(text, parse_constant=invalid_constant)


def decode_json_cells_csv(text: str) -> list[dict[str, Any]]:
    reader = csv.DictReader(io.StringIO(text, newline=""))
    headers = reader.fieldnames or []
    require(bool(headers) and len(headers) == len(set(headers)), "Invalid/duplicate CSV headers")
    rows = []
    for raw in reader:
        require(None not in raw and all(v is not None for v in raw.values()), "CSV row has wrong field count")
        rows.append({key: load_json(value) for key, value in raw.items() if value != ""})
        require(len(rows) <= 500, "CSV exceeds 500 rows")
    return rows


@contextmanager
def checked_zip(path: Path):
    require(path.stat().st_size <= TOTAL, "Compressed package exceeds 100 MiB")
    with zipfile.ZipFile(path) as zf:
        infos = zf.infolist()
        require(len(infos) <= 1000, "ZIP exceeds 1000 entries")
        seen = set()
        expanded = metadata = 0
        for info in infos:
            safe_path(info.filename)
            require(info.filename not in seen, "Duplicate ZIP member")
            seen.add(info.filename)
            require(stat.S_IFMT(info.external_attr >> 16) != stat.S_IFLNK, "ZIP symlinks are not supported")
            require(not info.flag_bits & 1, "Encrypted ZIP members are not supported")
            require(info.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), "Unsupported ZIP compression")
            expanded += info.file_size
            if not info.is_dir():
                if info.filename.startswith("assets/"):
                    require(info.file_size <= MEDIA, "Asset exceeds 20 MiB")
                else:
                    metadata += info.file_size
        require(expanded <= TOTAL, "Expanded package exceeds 100 MiB")
        require(metadata <= METADATA, "Package metadata exceeds 700 KiB")
        yield zf


def read_table(zf: zipfile.ZipFile, path: str) -> list[dict[str, Any]]:
    return decode_json_cells_csv(zf.read(path).decode("utf-8-sig"))


def inspect(zf: zipfile.ZipFile) -> dict[str, Any]:
    manifest = load_json(zf.read("manifest.json").decode("utf-8-sig"))
    require(isinstance(manifest, dict), "Manifest must be an object")
    require(manifest.get("format") == "mnemodim" and type(manifest.get("version")) is int
            and manifest["version"] == 1, "Unsupported manifest format/version")
    require(manifest.get("csvEncoding") == "json-cells-v1", "Unsupported CSV encoding")
    require(isinstance(manifest.get("name"), str), "Missing package name")
    palaces, assets, sheets = (manifest.get(k, []) for k in ("palaces", "assets", "sheets"))
    require(all(isinstance(v, list) for v in (palaces, assets, sheets)), "Invalid manifest lists")
    require(1 <= len(palaces) <= 25 and len(assets) <= 200 and len(sheets) <= 500, "Manifest limits exceeded")
    require(manifest.get("kind") in ("palace", "collection"), "Invalid package kind")
    require(manifest["kind"] != "palace" or len(palaces) == 1, "A palace package must contain one palace")
    names = set(zf.namelist())
    expected = {"manifest.json"}
    asset_ids = set()
    for asset in assets:
        aid = identifier(asset["id"])
        require(aid not in asset_ids, "Duplicate asset ID")
        asset_ids.add(aid)
        path = f"assets/{aid}"
        require(asset.get("path") == path and path in names, "Missing/invalid asset path")
        require(asset.get("mime") in EXTENSIONS, "Unsupported media MIME (SVG must be rasterized)")
        require(type(asset.get("size")) is int and asset["size"] == zf.getinfo(path).file_size
                and asset["size"] > 0, "Asset size mismatch/empty media")
        expected.add(path)

    def media_reference(row, keys):
        for key in keys:
            value = row.get(key)
            require(value in (None, "") or value in asset_ids, f"Unknown media reference: {key}")

    palace_ids, stage_ids, locus_ids = set(), set(), set()
    all_stages, all_loci, all_rows = [], [], []
    for palace in palaces:
        pid = identifier(palace["id"])
        require(pid not in palace_ids, "Duplicate palace ID")
        palace_ids.add(pid)
        media_reference(palace, ("background", "thumbnailImagePath"))
        stage_path, locus_path = f"palaces/{pid}/stages.csv", f"palaces/{pid}/loci.csv"
        expected.update((stage_path, locus_path))
        stages, loci = read_table(zf, stage_path), read_table(zf, locus_path)
        local_stages = set()
        for stage in stages:
            sid = identifier(stage["id"])
            require(sid not in stage_ids and stage.get("palaceId") == pid, "Duplicate stage or invalid palace reference")
            stage_ids.add(sid)
            local_stages.add(sid)
            media_reference(stage, ("imagePath",))
        for locus in loci:
            lid = identifier(locus["id"])
            require(lid not in locus_ids and locus.get("palaceId") == pid
                    and locus.get("stageId") in local_stages, "Duplicate locus or invalid stage/palace reference")
            locus_ids.add(lid)
            for key in ("x", "y"):
                value = locus.get(key)
                require(type(value) in (int, float) and 0 <= value <= 1, "Locus x/y must be normalized 0..1")
            media_reference(locus, ("imagePath", "soundPath"))
        all_stages.extend(stages)
        all_loci.extend(loci)
    require(manifest.get("entryPalaceId") in palace_ids, "Invalid entryPalaceId")
    require(len(all_stages) <= 100 and len(all_loci) <= 500, "Stage/locus limits exceeded")
    identities = set()
    for sheet in sheets:
        name, pid = sheet["sheet"], sheet.get("palaceId")
        require(isinstance(name, str) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,63}", name) is not None, "Invalid sheet name")
        require(pid is None or pid in palace_ids, "Unknown table palace")
        path = f"palaces/{pid}/tables/{name}.csv" if pid is not None else f"shared/tables/{name}.csv"
        require(sheet.get("path") == path and path not in expected, "Invalid/duplicate sheet path")
        expected.add(path)
        rows = read_table(zf, path)
        for row in rows:
            require(row.get("sheet") == name and row.get("palaceId") == pid, "Table row scope mismatch")
            key = row["key"]
            require(isinstance(key, str) and 1 <= len(key) <= 64, "Invalid table key")
            identity = (pid, name, key)
            require(identity not in identities, "Duplicate table row identity")
            identities.add(identity)
            cells = row.get("cells")
            require(isinstance(cells, list) and len(cells) <= 24, "Invalid table cells")
            for cell in cells:
                kind, value = cell.get("type"), cell.get("value")
                require(kind in ("text", "number", "boolean", "json", "formula", "asset", "palace", "stage"), "Invalid cell type")
                if kind in ("asset", "palace", "stage") and value not in (None, ""):
                    allowed = {"asset": asset_ids, "palace": palace_ids, "stage": stage_ids}[kind]
                    require(value in allowed, "Unknown table reference")
        all_rows.extend(rows)
    require(len(all_rows) <= 500, "Workbook exceeds 500 rows")
    require({i.filename for i in zf.infolist() if not i.is_dir()} == expected, "Missing or unlisted ZIP files")
    return {"manifest": manifest, "stages": all_stages, "loci": all_loci, "rows": all_rows}


def summary(path: Path) -> int:
    with checked_zip(path) as zf:
        data = inspect(zf)
    manifest = data["manifest"]
    print(f"file: {path}")
    print(f"name: {manifest['name'][:160]!r}  kind: {manifest['kind']}  entry: {manifest['entryPalaceId']}")
    print(f"palaces={len(manifest['palaces'])} stages={len(data['stages'])} loci={len(data['loci'])} "
          f"tables={len(manifest.get('sheets', []))} rows={len(data['rows'])} assets={len(manifest.get('assets', []))}")
    for locus in data["loci"][:8]:
        print(f"locus {locus['id']}: stage={locus['stageId']} label={str(locus.get('label', ''))[:80]!r} "
              f"memory={str(locus.get('memory', ''))[:80]!r} xy=({locus['x']},{locus['y']})")
    print("PASS: structural checks only; media decoding, formulas and application import NOT tested.")
    return 0


def extract_assets(path: Path, out: Path) -> int:
    with checked_zip(path) as zf:
        data = inspect(zf)
        # mkdir is exclusive: preexisting directories, files and symlinks fail.
        out.mkdir(parents=False, exist_ok=False)
        try:
            for asset in data["manifest"].get("assets", []):
                dest = out / (asset["id"] + EXTENSIONS[asset["mime"]])
                with dest.open("xb") as target:
                    target.write(zf.read(asset["path"]))
                print(dest)
        except BaseException:
            shutil.rmtree(out)
            raise
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)
    command = sub.add_parser("summary", help="read-only summary and structural checks")
    command.add_argument("file", type=Path)
    command = sub.add_parser("extract-assets", help="extract media into a NEW directory; no overwrites")
    command.add_argument("file", type=Path)
    command.add_argument("out", type=Path)
    args = parser.parse_args()
    try:
        if args.cmd == "summary":
            return summary(args.file)
        return extract_assets(args.file, args.out)
    except (OSError, ValueError, KeyError, TypeError, AttributeError, csv.Error, zipfile.BadZipFile, RuntimeError) as error:
        print(f"ERROR: {str(error)[:400]}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
