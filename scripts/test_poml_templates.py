#!/usr/bin/env python3
"""Strictly render every shipped Praxis template against synthetic contexts.

No fallback, providers, secrets, live databases or state changes. Includes render
from their actual paths while the process cwd is a separate temporary directory.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cli', default=os.environ.get('POML_CLI'))
    parser.add_argument('--context', type=Path, default=ROOT / 'examples/poml-test-context.json')
    parser.add_argument('--context-case', choices=['full', 'sparse', 'null', 'chat'], action='append')
    parser.add_argument('--templates-dir', type=Path, default=ROOT / 'templates', help='Template tree to validate, including a repaired installation')
    parser.add_argument('--template', action='append', help='Optional name relative to --templates-dir, including .poml')
    args = parser.parse_args()
    if not args.cli or not Path(args.cli).is_file():
        parser.error('Set POML_CLI or --cli to the installed Microsoft JavaScript CLI')
    full = json.loads(args.context.read_text(encoding='utf-8'))
    source = (ROOT / 'src/db/contexts.rs').read_text(encoding='utf-8')
    for struct, values in [('Context', full), ('ContextSettings', full.get('settings', {}))]:
        body = re.search(r'pub struct ' + struct + r'\s*\{(.*?)\n\}', source, re.S).group(1)
        fields = set(re.findall(r'^\s*pub (\w+):', body, re.M))
        missing = fields - set(values)
        if missing:
            parser.error(f'Full fixture is missing {struct} fields: {sorted(missing)}')
        print(f'PASS fixture coverage: {len(fields)} {struct} fields')
    cases = {'full': full, 'sparse': {}, 'null': {k: None for k in full},
             'chat': {**copy.deepcopy(full), 'mode': 'chat'}}
    selected_cases = args.context_case or list(cases)
    template_root = args.templates_dir.resolve()
    if not template_root.is_dir():
        parser.error(f'Missing template directory: {template_root}')
    templates = [template_root / name for name in args.template] if args.template else sorted(template_root.rglob('*.poml'))
    failures = []
    count = 0
    with tempfile.TemporaryDirectory(prefix='praxis-all-poml-') as temp:
        ctx_file = Path(temp) / 'context.json'
        for template in templates:
            for label in selected_cases:
                ctx_file.write_text(json.dumps(cases[label], ensure_ascii=False), encoding='utf-8')
                result = subprocess.run(['node', str(Path(args.cli).resolve()), '--file', str(template),
                                         '--context-file', str(ctx_file), '--strict', '--speakerMode=false'],
                                        capture_output=True, text=True, timeout=45, cwd=temp)
                name = str(template.relative_to(template_root))
                try:
                    assert result.returncode == 0, result.stderr[:1000]
                    output = json.loads(result.stdout)['messages']
                    assert isinstance(output, str) and output.strip(), 'Empty/non-text output'
                    if label in ('full', 'chat'):
                        if name == 'discovery/skills.poml':
                            plan = json.loads(output)
                            assert plan['names'] == [] and plan['queries'] == []
                            assert 'search_skills' in plan['instructions']
                        elif name == 'compaction.poml':
                            assert 'CONVERSATION_SENTINEL' in output
                        elif name != 'shared/blueprint.poml' and name != 'shared/task_inputs.poml':
                            assert 'POML_CURRENT_INPUT_SENTINEL' in output, 'Current user input was omitted'
                        if name in ('standard.poml', 'system.poml', 'language_instructor.poml', 'language_learning.poml', 'code_assistant.poml', 'researcher.poml'):
                            for sentinel in ('Onyx', 'Red Ball', 'Grandfather Clock', 'MEMORY_FACT_SENTINEL', 'poml_templates', 'use_skill'):
                                assert sentinel in output, f'Missing semantic/runtime content: {sentinel}'
                    print(f'PASS {name}: {label}')
                    count += 1
                except (AssertionError, ValueError, KeyError) as error:
                    failures.append(f'{name} [{label}]: {error}')
                    print(f'FAIL {name}: {label}')
    if failures:
        parser.exit(1, '\n'.join(failures) + '\n')
    print(f'PASS: {count} strict renders across {len(templates)} templates and {len(selected_cases)} context cases.')


if __name__ == '__main__':
    main()
