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
                         'entryPalaceId': 'p0', 'palaces': [{'id': 'p0', 'name': 'Palace', 'background': ''}],
                         'assets': [{'id': 'a0', 'path': 'assets/a0', 'mime': 'image/png', 'size': 12}],
                         'csvEncoding': 'json-cells-v1',
                         'sheets': [{'path': 'shared/tables/Major.csv', 'palaceId': None, 'sheet': 'Major'}]}
        self.files = {
            'palaces/p0/stages.csv': table([{'id': 's0', 'palaceId': 'p0', 'name': 'Room', 'orderIndex': 0, 'imagePath': 'a0'}],
                                         mod.STAGE_COLUMNS),
            'palaces/p0/loci.csv': table([{'id': 'l0', 'palaceId': 'p0', 'stageId': 's0', 'label': 'Tür',
                                         'memory': '日本語\n"quote", comma', 'x': .3, 'y': .4, 'contentType': 'recall'}],
                                       mod.LOCUS_COLUMNS),
            'shared/tables/Major.csv': table([{'id': 'r0', 'palaceId': None, 'sheet': 'Major', 'key': '07',
                                             'cells': [{'column': 'object', 'type': 'text', 'value': 'Tee'}]}],
                                           mod.ROW_COLUMNS),
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
            row = {'id': 'l0', 'palaceId': 'p0', 'stageId': 's0', 'label': 'Door', 'memory': 'Answer',
                   'contentType': 'recall', 'x': .3, 'y': .4, **changes}
            self.files['palaces/p0/loci.csv'] = table([row], mod.LOCUS_COLUMNS)
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

    def replace_row(self, path, columns, **changes):
        row = mod.decode_json_cells_csv(self.files[path])[0]
        row.update(changes)
        self.files[path] = table([row], columns)

    def test_cell_values_are_strings_without_corrupting_positive_cases(self):
        cells = [
            {'column': 'order', 'type': 'number', 'value': '1'},
            {'column': 'enabled', 'type': 'boolean', 'value': 'true'},
            {'column': 'items', 'type': 'json', 'value': '[1,2]'},
            {'column': 'settings', 'type': 'json', 'value': '{"enabled":true,"count":3}'},
            {'column': 'nothing', 'type': 'json', 'value': 'null'},
            {'column': 'digits', 'type': 'text', 'value': '007'},
            {'column': 'unicode', 'type': 'text', 'value': '日本語 😀\n"quoted", comma'},
        ]
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS, cells=cells)
        self.assertEqual(self.inspect()['rows'][0]['cells'], cells)
        self.assertEqual(self.inspect()['stages'][0]['orderIndex'], 0)
        self.assertEqual(self.inspect()['loci'][0]['x'], .3)
        self.assertIsNone(self.inspect()['rows'][0]['palaceId'])
        for kind, value in [('number', 1), ('boolean', True), ('json', {}), ('json', []),
                            ('text', None), ('text', 'x' * 16001), ('text', 'a\0b'),
                            ('text', '😀' * 8001)]:
            with self.subTest(kind=kind, value_type=type(value).__name__):
                self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS,
                                 cells=[{'column': 'order', 'type': kind, 'value': value}])
                with self.assertRaisesRegex(ValueError, r'shared/tables/Major.csv: row r0 Major.07.*order.*Invalid cell'):
                    self.inspect()
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS,
                         cells=[{'column': 'order', 'type': 'number'}])
        with self.assertRaisesRegex(ValueError, 'Invalid cell'):
            self.inspect()

    def test_cell_type_content_and_column_errors(self):
        for kind, value, error in [
            ('number', '', 'Invalid numeric cell'),
            ('number', 'seven', 'Invalid numeric cell'),
            ('number', '1_000', 'Invalid numeric cell'),
            ('number', '1,5', 'Invalid numeric cell'),
            ('number', '1e400', 'Invalid numeric cell'),
            ('number', '9007199254740992', 'Invalid numeric cell'),
            ('boolean', 'True', 'Boolean cells must be true or false'),
            ('json', '{a:1}', 'Expecting property name'),
            ('json', '{"nested":{"constructor":1}}', 'Forbidden JSON key'),
            ('json', '[1e400]', 'Invalid JSON number'),
            ('json', '[' * 25 + '0' + ']' * 25, 'JSON cell is too complex'),
            ('json', '[' + ','.join('0' for _ in range(4000)) + ']', 'JSON cell is too complex'),
            ('integer', '1', 'Invalid cell type'),
            ('formula', 'x' * 2049, 'Invalid formula expression'),
        ]:
            with self.subTest(kind=kind, value=value[:40]):
                with self.assertRaisesRegex(ValueError, error):
                    mod.validate_cell({'column': 'value', 'type': kind, 'value': value}, set(), 'fixture')
        for value in ['1', '-1.5', '1e2', '0x10', '0b10', '0o10', ' 7 ', '.5', '1.']:
            mod.validate_cell({'column': 'value', 'type': 'number', 'value': value}, set(), 'fixture')
        for column in ['constructor', '1bad', 'duplicate']:
            with self.assertRaisesRegex(ValueError, 'Invalid or duplicate column'):
                mod.validate_cell({'column': column, 'type': 'text', 'value': ''}, {'duplicate'}, 'fixture')
        with self.assertRaisesRegex(ValueError, 'Unknown/missing cell field'):
            mod.validate_cell({'column': 'value', 'type': 'text', 'value': '', 'invented': 1}, set(), 'fixture')

    def test_valid_large_cells_array_and_utf16_boundary(self):
        cells = [{'column': f'c{i}', 'type': 'text', 'value': 'x' * 16000} for i in range(23)]
        cells.append({'column': 'unicode', 'type': 'text', 'value': '😀' * 8000})
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS, cells=cells)
        previous = csv.field_size_limit()
        self.assertEqual(self.inspect()['rows'][0]['cells'], cells)
        self.assertEqual(csv.field_size_limit(), previous)
        with self.assertRaises(ValueError):
            mod.decode_json_cells_csv('id\nnot-json\n')
        self.assertEqual(csv.field_size_limit(), previous)

    def test_complete_headers_required_even_for_absent_optional_values(self):
        path = 'palaces/p0/loci.csv'
        row = mod.decode_json_cells_csv(self.files[path])[0]
        for columns in [list(row), list(reversed(mod.LOCUS_COLUMNS)), mod.LOCUS_COLUMNS + ['invented']]:
            self.files[path] = table([row], columns)
            with self.assertRaisesRegex(ValueError, 'Unexpected CSV columns'):
                self.inspect()
        self.files[path] = '\ufeff' + table([row], mod.LOCUS_COLUMNS)
        with self.assertRaisesRegex(ValueError, 'Unexpected CSV columns'):
            self.inspect()

    def test_required_background_and_stage_order(self):
        del self.manifest['palaces'][0]['background']
        with self.assertRaisesRegex(ValueError, 'Invalid background'):
            self.inspect()
        self.manifest['palaces'][0]['background'] = ''
        for value in ['0', True, -1, .5, mod.MAX_SAFE + 1]:
            self.replace_row('palaces/p0/stages.csv', mod.STAGE_COLUMNS, orderIndex=value)
            with self.assertRaisesRegex(ValueError, '[Ss]tage order'):
                self.inspect()

    def test_global_ids_and_required_shared_scope(self):
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS, id='s0')
        with self.assertRaisesRegex(ValueError, 'duplicate portable ID'):
            self.inspect()
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS, id='r0')
        row = mod.decode_json_cells_csv(self.files['shared/tables/Major.csv'])[0]
        del row['palaceId']
        self.files['shared/tables/Major.csv'] = table([row], mod.ROW_COLUMNS)
        with self.assertRaisesRegex(ValueError, 'shared palaceId must be null'):
            self.inspect()
        self.files['shared/tables/Major.csv'] = table([], mod.ROW_COLUMNS)
        with self.assertRaisesRegex(ValueError, 'empty custom table'):
            self.inspect()

    def test_quiz_fallback_and_bindings(self):
        path = 'palaces/p0/loci.csv'
        for memory in ['', ' , ', ','.join(map(str, range(21)))]:
            self.replace_row(path, mod.LOCUS_COLUMNS, contentType='quiz', memory=memory,
                             bindings=[{'field': 'memory', 'expression': 'Inputs.answer'}])
            with self.assertRaisesRegex(ValueError, 'Invalid quiz answers'):
                self.inspect()
        self.replace_row(path, mod.LOCUS_COLUMNS, memory='a, A, b')
        self.inspect()
        self.replace_row(path, mod.LOCUS_COLUMNS,
                         bindings=[{'field': 'active', 'expression': 'true'}] * 2)
        with self.assertRaisesRegex(ValueError, 'Duplicate binding field'):
            self.inspect()
        self.replace_row(path, mod.LOCUS_COLUMNS, bindings=[{'field': 'x', 'expression': 'x' * 2049}])
        with self.assertRaisesRegex(ValueError, 'Invalid expression'):
            self.inspect()

    def test_directory_entries_rejected(self):
        self.files['palaces/'] = b''
        with self.assertRaisesRegex(ValueError, 'directory entries'):
            self.inspect()

    def test_metadata_entry_budget_is_decimal_not_aggregate(self):
        self.assertEqual(mod.METADATA, 700_000)
        self.files['first.csv'] = b'x' * 400_000
        self.files['second.csv'] = b'x' * 400_000
        self.write()
        with mod.checked_zip(self.path):
            pass  # This is ZIP preflight, not reconstructed-document validation.
        self.files['first.csv'] = b'x' * 700_001
        self.write()
        with self.assertRaisesRegex(ValueError, 'metadata entry'):
            with mod.checked_zip(self.path):
                pass

    def test_formulas_are_not_misreported_as_validated(self):
        self.replace_row('shared/tables/Major.csv', mod.ROW_COLUMNS,
                         cells=[{'column': 'value', 'type': 'formula', 'value': '1+'}])
        self.write()
        result = subprocess.run([sys.executable, str(HELPER), 'summary', str(self.path)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('formula syntax/runtime and application/backend import NOT tested', result.stdout)
        self.assertIn('decodePackage + parseFormula', result.stdout)

    def test_guide_writer_example_passes_preflight(self):
        guide = (HELPER.parents[1] / 'references/MNEMODIM_IMPORT_GUIDE.md').read_text(encoding='utf-8')
        source = guide.split('```python\n', 1)[1].split('\n```', 1)[0]
        script = self.root / 'example.py'
        script.write_text(source, encoding='utf-8')
        output = self.root / 'example.mnemodim'
        result = subprocess.run([sys.executable, str(script), str(output)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        with mod.checked_zip(output) as zf:
            data = mod.inspect(zf)
        self.assertEqual([c['value'] for c in data['rows'][0]['cells']], ['1', 'true', '[1,2]'])
        before = output.read_bytes()
        result = subprocess.run([sys.executable, str(script), str(output)],
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(output.read_bytes(), before)

    def test_bad_cli_exits_nonzero(self):
        self.path.write_text('not a zip')
        result = subprocess.run([sys.executable, str(HELPER), 'summary', str(self.path)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 1)
        self.assertIn('ERROR:', result.stderr)
        self.assertNotIn('Traceback', result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
