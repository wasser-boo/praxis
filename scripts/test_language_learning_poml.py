#!/usr/bin/env python3
"""Render the tutor POML locally, optionally against a read-only saved context."""
import argparse
import copy
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cli', default=os.environ.get('POML_CLI'), required=not bool(os.environ.get('POML_CLI')))
parser.add_argument('--template', type=Path, action='append')
parser.add_argument('--context-db', type=Path)
parser.add_argument('--user-id')
args = parser.parse_args()
templates = args.template or [Path(__file__).resolve().parents[1] / 'templates/language_learning.poml']
cases = [('empty context', {}, None)]
if args.context_db:
    conn = sqlite3.connect(args.context_db.resolve().as_uri() + '?mode=ro', uri=True)
    conn.execute('PRAGMA query_only=ON')
    if args.user_id:
        rows = conn.execute('SELECT data FROM contexts WHERE user_id=?', (args.user_id,)).fetchall()
    else:
        rows = conn.execute('SELECT data FROM contexts').fetchall()
    conn.close()
    if len(rows) != 1:
        raise SystemExit('Specify --user-id: expected exactly one matching context')
    saved = json.loads(rows[0][0])
    cases.append(('saved deployment context (unchanged)', saved, None))
else:
    saved = {'settings': {}, 'custom_data': {}, 'sm_data': {}}
for mode in ('chat', 'agent'):
    for label, value in [('absent', None), ('empty', ''), ('French', 'Bonjour, aide-moi en allemand.'), ('Japanese', '日本語を練習したいです。')]:
        context = copy.deepcopy(saved)
        context['mode'] = mode
        context.pop('user_prompt', None)
        if value is not None:
            context['user_prompt'] = value
        cases.append((f'{mode}: {label} user_prompt', context, value))

count = 0
with tempfile.TemporaryDirectory(prefix='praxis-poml-test-') as folder:
    for template in templates:
        if not template.is_file():
            raise SystemExit(f'Missing template: {template}')
        for label, context, expected in cases:
            context_file = Path(folder) / 'context.json'
            context_file.write_text(json.dumps(context, ensure_ascii=False))
            result = subprocess.run(['node', str(Path(args.cli).resolve()), '--file', str(template.resolve()),
                                     '--context-file', str(context_file), '--prettyPrint'],
                                    text=True, capture_output=True, timeout=60)
            if result.returncode:
                # Do not print render context, stored messages or raw renderer output.
                error = re.search(r'(?:ReadError|ReferenceError|TypeError):[^\n]*', result.stderr)
                raise SystemExit(f'FAIL {template.name}, {label}: ' + (error[0] if error else f'CLI exit {result.returncode}'))
            output = result.stdout
            try:
                output = json.dumps(json.loads(output), ensure_ascii=False)
            except json.JSONDecodeError:
                pass
            if not all(language in output for language in ('French', 'Japanese', 'German')):
                raise SystemExit(f'FAIL {label}: tutor instructions are missing')
            if expected and expected not in output:
                raise SystemExit(f'FAIL {label}: populated current request was not rendered')
            print(f'PASS {template.name}: {label}')
            count += 1
print(f'PASS: {count} local POML renders; no API calls or database changes.')
