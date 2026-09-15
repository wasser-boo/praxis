#!/usr/bin/env python3
"""Strict, synthetic POML regression: bounded discovery and durable tutor memory."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
cli = Path(os.environ['POML_CLI']).resolve()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--templates-dir', type=Path, default=ROOT / 'templates')
args = parser.parse_args()
templates = args.templates_dir.resolve()
context = json.loads((ROOT / 'examples/poml-test-context.json').read_text())
context['utc_now'] = '2026-09-14T12:00:00Z'
context['tools'] = [{'name': f'BULK_TOOL_{i}', 'description': 'CATALOG_DUMP_SENTINEL'} for i in range(100)]
context['skills'] = [{'name': f'BULK_SKILL_{i}', 'description': 'metadata', 'required_parameters': []} for i in range(100)]
context['memory']['variables'] = {
    'srs_items': [
        {'id': 'past', 'language': 'French', 'item': 'DUE_WORD_SENTINEL', 'translation': 'past', 'due': '2026-09-13', 'interval_days': 1},
        {'id': 'future', 'language': 'French', 'item': 'FUTURE_WORD_SENTINEL', 'due': '2099-01-01'},
        {'id': 'other', 'language': 'Japanese', 'item': 'OTHER_LANGUAGE_SENTINEL', 'due': '2026-01-01'},
        None,
    ], 'xp': 12, 'learning_profile': {'target_language': 'French', 'level': 'A2'},
}
with tempfile.TemporaryDirectory() as directory:
    file = Path(directory) / 'context.json'
    file.write_text(json.dumps(context))
    for name in ['standard', 'system', 'code_assistant', 'code_reviewer', 'researcher', 'language_instructor', 'language_learning', 'tasks/daily_quiz', 'tasks/transcript_check', 'daily_quiz', 'transcript_check']:
        proc = subprocess.run(['node', str(cli), '--file', str(templates / (name + '.poml')), '--context-file', str(file), '--strict', '--speakerMode=false'], capture_output=True, text=True, check=True, cwd=directory, timeout=45)
        output = json.loads(proc.stdout)['messages']
        assert 'search_tools' in output, name
        assert 'search_skills' in output, name
        assert 'memory_profile_create' in output and 'memory_profile_load' in output, name
        assert 'shared' in output and 'expected_profile' in output, name
        assert 'CATALOG_DUMP_SENTINEL' not in output, name
        assert 'BULK_SKILL_99' not in output, name
        if name in ['language_instructor', 'language_learning', 'tasks/daily_quiz', 'daily_quiz']:
            assert 'memory_get' in output and 'memory_set' in output, name
            assert 'set_context to memory' not in output and 'key memory.variables' not in output, name
            assert 'DUE_WORD_SENTINEL' in output, name
            assert 'FUTURE_WORD_SENTINEL' not in output and 'OTHER_LANGUAGE_SENTINEL' not in output, name
            assert 'expected_value' in output, name
        print('PASS', name)
print('PASS progressive discovery and SRS memory prompts; no live APIs')
