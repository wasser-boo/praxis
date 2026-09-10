#!/usr/bin/env python3
"""Offline synthetic ZIP regressions for the bundled mnemodim helper; no real palaces touched."""
import csv
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

HELPER = Path(__file__).resolve().parents[1] / 'skills/mnemodim-palace/scripts/mnemodim_tool.py'
spec = importlib.util.spec_from_file_location('mnemodim_tool', HELPER)
mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mod)


def table(rows, columns):
    out = io.StringIO(newline='')
    writer = csv.writer(out)
    writer.writerow(columns)
    for row in rows:
        writer.writerow([json.dumps(row[k], ensure_ascii=False) if k in row else '' for k in columns])
    return out.getvalue()


class MnemodimTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.path = self.root / 'synthetic.mnemodim'
        self.manifest = {'format': 'mnemodim', 'version': 1, 'kind': 'palace', 'name': 'Übung 日本語',
                         'entryPalaceId': 'p0', 'palaces': [{'id': 'p0', 'name': 'Palace'}],
                         'assets': [{'id': 'a0', 'path': 'assets/a0', 'mime': 'image/png', 'size': 12}],
                         'csvEncoding': 'json-cells-v1',
                         'sheets': [{'path': 'shared/tables/Major.csv', 'palaceId': None, 'sheet': 'Major'}]}
        self.files = {
            'palaces/p0/stages.csv': table([{'id': 's0', 'palaceId': 'p0', 'name': 'Room', 'orderIndex': 0, 'imagePath': 'a0'}],
                                         ['id', 'palaceId', 'name', 'orderIndex', 'imagePath']),
            'palaces/p0/loci.csv': table([{'id': 'l0', 'palaceId': 'p0', 'stageId': 's0', 'label': 'Tür',
                                         'memory': '日本語\n"quote", comma', 'x': .3, 'y': .4}],
                                       ['id', 'palaceId', 'stageId', 'label', 'memory', 'x', 'y', 'notes']),
            'shared/tables/Major.csv': table([{'id': 'r0', 'sheet': 'Major', 'key': '07',
                                             'cells': [{'column': 'object', 'type': 'text', 'value': 'Tee'}]}],
                                           ['id', 'palaceId', 'sheet', 'key', 'cells']),
            'assets/a0': b'\x89PNG\r\n\x1a\nTEST',
        }

    def write(self):
        with zipfile.ZipFile(self.path, 'w', compression=zipfile.ZIP_DEFLATED) as zf:
            zf.writestr('manifest.json', json.dumps(self.manifest, ensure_ascii=False))
            for name, data in self.files.items():
                zf.writestr(name, data)

    def inspect(self):
        self.write()
        with mod.checked_zip(self.path) as zf:
            return mod.inspect(zf)

    def test_roundtrip_json_cells_and_cli_from_other_cwd(self):
        data = self.inspect()
        self.assertEqual(data['loci'][0]['memory'], '日本語\n"quote", comma')
        self.assertNotIn('notes', data['loci'][0])
        self.assertEqual(data['rows'][0]['cells'][0]['value'], 'Tee')
        before = self.path.read_bytes()
        result = subprocess.run([sys.executable, str(HELPER), 'summary', str(self.path)], cwd=self.root,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('stages=1 loci=1', result.stdout)
        self.assertIn('NOT tested', result.stdout)
        self.assertEqual(self.path.read_bytes(), before)

    def test_extract_is_exclusive(self):
        self.write()
        out = self.root / 'assets'
        mod.extract_assets(self.path, out)
        self.assertEqual((out / 'a0.png').read_bytes(), self.files['assets/a0'])
        with self.assertRaises(FileExistsError):
            mod.extract_assets(self.path, out)
        self.assertEqual((out / 'a0.png').read_bytes(), self.files['assets/a0'])

    def test_asset_id_path_traversal(self):
        self.manifest['assets'][0]['id'] = '../escape'
        with self.assertRaises(ValueError):
            self.inspect()
        self.assertFalse((self.root.parent / 'escape.png').exists())

    def test_member_path_traversal_and_duplicate(self):
        for path in ('../escape', '/absolute', 'assets/../../escape', 'assets\\escape'):
            self.files[path] = b'x'
            with self.assertRaises(ValueError):
                self.inspect()
            del self.files[path]
        self.write()
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            with zipfile.ZipFile(self.path, 'a') as zf:
                zf.writestr('manifest.json', '{}')
        with self.assertRaises(ValueError):
            with mod.checked_zip(self.path):
                pass

    def test_symlink_and_missing_asset(self):
        self.write()
        info = zipfile.ZipInfo('assets/link')
        info.create_system = 3
        info.external_attr = 0o120777 << 16
        with zipfile.ZipFile(self.path, 'a') as zf:
            zf.writestr(info, '../outside')
        with self.assertRaises(ValueError):
            with mod.checked_zip(self.path):
                pass
        del self.files['assets/a0']
        with self.assertRaises(ValueError):
            self.inspect()

    def test_wrong_owner_or_coordinate(self):
        for changes in ({'stageId': 'missing'}, {'palaceId': 'another'}, {'x': 2}, {'x': True}):
            row = {'id': 'l0', 'palaceId': 'p0', 'stageId': 's0', 'x': .3, 'y': .4, **changes}
            self.files['palaces/p0/loci.csv'] = table([row], list(row))
            with self.assertRaises(ValueError):
                self.inspect()

    def test_archive_size_preflight(self):
        self.files['assets/a0'] = b'0' * (mod.MEDIA + 1)
        self.manifest['assets'][0]['size'] = len(self.files['assets/a0'])
        with self.assertRaisesRegex(ValueError, '20 MiB'):
            self.inspect()
        del self.files['assets/a0']
        self.files['large.csv'] = ' ' * (mod.METADATA + 1)
        with self.assertRaisesRegex(ValueError, 'metadata'):
            self.inspect()

    def test_invalid_json_cells_and_unlisted_entries(self):
        for text in ('id,id\n"1","2"\n', 'id\nnot-json\n', 'id\n1,2\n', 'id\nNaN\n'):
            with self.assertRaises(ValueError):
                mod.decode_json_cells_csv(text)
        self.files['unlisted.txt'] = b'not an instruction'
        with self.assertRaises(ValueError):
            self.inspect()

    def test_cleanup_after_extraction_failure(self):
        self.write()
        original = zipfile.ZipFile.read
        def read(zf, name, *args, **kwargs):
            if name == 'assets/a0':
                raise zipfile.BadZipFile('synthetic CRC failure')
            return original(zf, name, *args, **kwargs)
        out = self.root / 'partial'
        with patch.object(zipfile.ZipFile, 'read', read):
            with self.assertRaises(zipfile.BadZipFile):
                mod.extract_assets(self.path, out)
        self.assertFalse(out.exists())

    def test_bad_cli_exits_nonzero(self):
        self.path.write_text('not a zip')
        result = subprocess.run([sys.executable, str(HELPER), 'summary', str(self.path)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn('ERROR:', result.stderr)
        self.assertNotIn('Traceback', result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
