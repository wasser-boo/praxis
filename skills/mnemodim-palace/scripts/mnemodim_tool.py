#!/usr/bin/env python3
"""Read-only structural inspection / safe media extraction for .mnemodim v1.

Adapted from the Pi mnemodim-palace helper. Python 3.9+, standard library only.
Prevents common import errors from references/MNEMODIM_IMPORT_GUIDE.md (8276140).
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
METADATA = 700_000  # Per non-asset entry; not an aggregate CSV budget.
MAX_SAFE = 9_007_199_254_740_991
BLOCKED = {"__proto__", "prototype", "constructor"}
STAGE_COLUMNS = ["id", "palaceId", "name", "orderIndex", "imagePath"]
LOCUS_COLUMNS = [
    "id", "palaceId", "stageId", "label", "memory", "x", "y", "zoom", "panX", "panY",
    "notes", "imagePath", "soundPath", "contentType", "description", "quiz", "alternatives", "bindings",
]
ROW_COLUMNS = ["id", "palaceId", "sheet", "key", "cells"]
BINDING_FIELDS = {"label", "memory", "description", "imagePath", "x", "y", "zoom", "visible", "active"}
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
    require(value not in BLOCKED, "Invalid or duplicate portable ID: reserved name")
    return value


def text(value: Any, location: str, limit: int = 16_000) -> str:
    require(isinstance(value, str) and "\x00" not in value
            and len(value.encode("utf-16-le", errors="surrogatepass")) // 2 <= limit,
            f"Invalid {location}: expected string of at most {limit} UTF-16 code units, no NUL")
    return value


def safe_number(value: Any, location: str, low: float = -MAX_SAFE, high: float = MAX_SAFE) -> None:
    require(type(value) in (int, float) and low <= value <= high, f"Invalid {location}")


def validate_json_cell(value: Any) -> None:
    pending = [(value, 0)]
    nodes = 0
    while pending:
        item, depth = pending.pop()
        nodes += 1
        require(nodes <= 4000 and depth <= 24, "JSON cell is too complex")
        if type(item) in (int, float):
            safe_number(item, "JSON number")
        elif isinstance(item, dict):
            require(not BLOCKED.intersection(item), "Forbidden JSON key")
            pending.extend((child, depth + 1) for child in item.values())
        elif isinstance(item, list):
            pending.extend((child, depth + 1) for child in item)


def validate_cell(cell: Any, columns: set[str], location: str) -> None:
    try:
        require(isinstance(cell, dict), "Expected a cell object")
        # The importer checks string storage before interpreting the declared type.
        value = text(cell.get("value"), "cell")
        require(set(cell) == {"column", "type", "value"}, "Unknown/missing cell field")
        column = text(cell["column"], "column", 64)
        require(re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,63}", column) is not None
                and column not in BLOCKED and column not in columns, "Invalid or duplicate column")
        columns.add(column)
        kind = cell["type"]
        require(kind in ("text", "number", "boolean", "json", "formula", "asset", "palace", "stage"),
                "Invalid cell type")
        if kind == "number":
            token = value.strip()
            # JavaScript Number accepts decimal and unsigned radix literals, not
            # Python underscores or locale commas. Never coerce the stored cell.
            if re.fullmatch(r"0(?:[xX][0-9a-fA-F]+|[bB][01]+|[oO][0-7]+)", token):
                number = int(token, 0)
            elif re.fullmatch(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?", token):
                number = float(token)
            else:
                raise ValueError("Invalid numeric cell")
            require(abs(number) <= MAX_SAFE, "Invalid numeric cell")
        elif kind == "boolean":
            require(value in ("true", "false"), "Boolean cells must be true or false")
        elif kind == "json":
            validate_json_cell(load_json(value))
        elif kind == "formula":
            text(value, "formula expression", 2048)  # Syntax still needs parseFormula.
    except (ValueError, RecursionError) as error:
        raise ValueError(f"{location}: {error}") from error


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


def decode_json_cells_csv(text: str, columns: list[str] | None = None) -> list[dict[str, Any]]:
    # One cells-array field may legitimately exceed Python's default 128 KiB.
    # Retain the package's bounded per-entry budget, restoring the global setting.
    previous_limit = csv.field_size_limit(METADATA)
    try:
        reader = csv.DictReader(io.StringIO(text, newline=""), strict=True)
        headers = reader.fieldnames or []
        require(bool(headers) and len(headers) == len(set(headers)), "Invalid/duplicate CSV headers")
        require(columns is None or headers == columns, "Unexpected CSV columns (use exact complete header order)")
        rows = []
        for raw in reader:
            require(None not in raw and all(v is not None for v in raw.values()), "CSV row has wrong field count")
            rows.append({key: load_json(value) for key, value in raw.items() if value != ""})
            require(len(rows) <= 500, "CSV exceeds 500 rows")
        return rows
    finally:
        csv.field_size_limit(previous_limit)


@contextmanager
def checked_zip(path: Path):
    require(path.stat().st_size <= TOTAL, "Compressed package exceeds 100 MiB")
    with zipfile.ZipFile(path) as zf:
        infos = zf.infolist()
        require(len(infos) <= 1000, "ZIP exceeds 1000 entries")
        seen = set()
        expanded = 0
        for info in infos:
            safe_path(info.filename)
            require(not info.is_dir(), "Explicit ZIP directory entries are unsupported")
            require(info.filename not in seen, "Duplicate ZIP member")
            seen.add(info.filename)
            require(stat.S_IFMT(info.external_attr >> 16) != stat.S_IFLNK, "ZIP symlinks are not supported")
            require(not info.flag_bits & 1, "Encrypted ZIP members are not supported")
            require(info.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED), "Unsupported ZIP compression")
            expanded += info.file_size
            if info.filename.startswith("assets/"):
                require(info.file_size <= MEDIA, "Asset exceeds 20 MiB")
            else:
                require(info.file_size <= METADATA, "ZIP metadata entry exceeds 700,000 bytes")
        require(expanded <= TOTAL, "Expanded package exceeds 100 MiB")
        yield zf


def read_table(zf: zipfile.ZipFile, path: str, columns: list[str]) -> list[dict[str, Any]]:
    try:
        return decode_json_cells_csv(zf.read(path).decode("utf-8"), columns)
    except (ValueError, csv.Error) as error:
        raise ValueError(f"{path}: {error}") from error


def inspect(zf: zipfile.ZipFile) -> dict[str, Any]:
    manifest = load_json(zf.read("manifest.json").decode("utf-8-sig"))
    require(isinstance(manifest, dict), "Manifest must be an object")
    require(manifest.get("format") == "mnemodim" and type(manifest.get("version")) is int
            and manifest["version"] == 1, "Unsupported manifest format/version")
    require(manifest.get("csvEncoding") == "json-cells-v1", "Unsupported CSV encoding")
    require(text(manifest.get("name"), "name").strip(), "Name is required")
    palaces, assets, sheets = (manifest.get(k) for k in ("palaces", "assets", "sheets"))
    require(all(isinstance(v, list) for v in (palaces, assets, sheets)), "Invalid manifest lists")
    require(1 <= len(palaces) <= 25 and len(assets) <= 200 and len(sheets) <= 500, "Manifest limits exceeded")
    require(manifest.get("kind") in ("palace", "collection"), "Invalid package kind")
    require(manifest["kind"] != "palace" or len(palaces) == 1, "A palace package must contain one palace")
    names = set(zf.namelist())
    expected = {"manifest.json"}
    portable_ids = set()

    def register(value):
        value = identifier(value)
        require(value not in portable_ids, f"Invalid or duplicate portable ID: {value}")
        portable_ids.add(value)
        return value

    asset_ids = set()
    for asset in assets:
        aid = register(asset["id"])
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
        pid = register(palace["id"])
        require(text(palace.get("name"), "palace name").strip(), "Palace name is required")
        text(palace.get("background"), "background")
        palace_ids.add(pid)
        media_reference(palace, ("background", "thumbnailImagePath"))
        stage_path, locus_path = f"palaces/{pid}/stages.csv", f"palaces/{pid}/loci.csv"
        expected.update((stage_path, locus_path))
        stages, loci = read_table(zf, stage_path, STAGE_COLUMNS), read_table(zf, locus_path, LOCUS_COLUMNS)
        local_stages = set()
        for stage in stages:
            sid = register(stage["id"])
            require(stage.get("palaceId") == pid, "Invalid stage palace reference")
            require(text(stage.get("name"), "stage name").strip(), "Stage name is required")
            order = stage.get("orderIndex")
            safe_number(order, "stage order", 0)
            require(order % 1 == 0, "Stage order must be an integer")
            stage_ids.add(sid)
            local_stages.add(sid)
            media_reference(stage, ("imagePath",))
        for locus in loci:
            lid = register(locus["id"])
            require(lid not in locus_ids and locus.get("palaceId") == pid
                    and locus.get("stageId") in local_stages, "Duplicate locus or invalid stage/palace reference")
            locus_ids.add(lid)
            for key in ("x", "y"):
                value = locus.get(key)
                require(type(value) in (int, float) and 0 <= value <= 1, "Locus x/y must be normalized 0..1")
            for key in ("label", "memory", "contentType"):
                text(locus.get(key), key)
            for key in ("notes", "imagePath", "soundPath", "description", "quiz"):
                if key in locus:
                    text(locus[key], key)
            for key in ("zoom", "panX", "panY"):
                if key in locus:
                    bounds = (0.01, 100) if key == "zoom" else (-MAX_SAFE, MAX_SAFE)
                    safe_number(locus[key], key, *bounds)
            if locus["contentType"] == "quiz":
                answers = {answer.strip().lower() for answer in locus["memory"].split(",") if answer.strip()}
                require(1 <= len(answers) <= 20, f"{locus_path}: {lid}: Invalid quiz answers")
            bindings = locus.get("bindings", [])
            require(isinstance(bindings, list) and len(bindings) <= 9, "Invalid bindings")
            fields = set()
            for binding in bindings:
                require(isinstance(binding, dict) and set(binding) == {"field", "expression"}, "Invalid binding")
                field = binding["field"]
                require(isinstance(field, str) and field in BINDING_FIELDS, "Unknown binding field")
                require(field not in fields, "Duplicate binding field")
                fields.add(field)
                text(binding["expression"], "expression", 2048)
            media_reference(locus, ("imagePath", "soundPath"))
        all_stages.extend(stages)
        all_loci.extend(loci)
    require(manifest.get("entryPalaceId") in palace_ids, "Invalid entryPalaceId")
    require(len(all_stages) <= 100 and len(all_loci) <= 500, "Stage/locus limits exceeded")
    identities = set()
    for sheet in sheets:
        name, pid = sheet["sheet"], sheet["palaceId"]
        require(isinstance(name, str) and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,63}", name) is not None
                and name not in BLOCKED and name != "View", "Invalid/reserved sheet name")
        require(pid is None or pid in palace_ids, "Unknown table palace")
        path = f"palaces/{pid}/tables/{name}.csv" if pid is not None else f"shared/tables/{name}.csv"
        require(sheet.get("path") == path and path not in expected, "Invalid/duplicate sheet path")
        expected.add(path)
        rows = read_table(zf, path, ROW_COLUMNS)
        require(bool(rows), f"{path}: CSV sheet scope mismatch (empty custom table)")
        for row in rows:
            register(row["id"])
            require(set(row) == set(ROW_COLUMNS), f"{path}: Missing table row fields (shared palaceId must be null)")
            require(row["sheet"] == name and row["palaceId"] == pid, "Table row scope mismatch")
            key = text(row["key"], "row key", 64)
            require(bool(key) and key not in BLOCKED, "Invalid row key")
            identity = (pid, name, key)
            require(identity not in identities, "Duplicate table row identity")
            identities.add(identity)
            cells = row.get("cells")
            require(isinstance(cells, list) and len(cells) <= 24, "Invalid table cells")
            columns = set()
            for index, cell in enumerate(cells):
                column = cell.get("column", "?") if isinstance(cell, dict) else "?"
                location = f"{path}: row {row['id']} {name}.{key}, cell[{index}] column {column!r}"
                validate_cell(cell, columns, location)
                kind, value = cell["type"], cell["value"]
                if kind in ("palace", "stage") or (kind == "asset" and value != ""):
                    allowed = {"asset": asset_ids, "palace": palace_ids, "stage": stage_ids}[kind]
                    require(value in allowed, f"{location}: Unknown table reference")
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
    print("PASS: bounded structural/cell preflight only. Full ZIP/media integrity, reconstructed document size, "
          "formula syntax/runtime and application/backend import NOT tested.")
    print("Before hand-off: follow references/MNEMODIM_IMPORT_GUIDE.md sections 7–8; "
          "use the real decodePackage + parseFormula checks when available.")
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
