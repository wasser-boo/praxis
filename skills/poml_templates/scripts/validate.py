#!/usr/bin/env python3
"""Strictly render a local POML template with synthetic JSON contexts (no LLM)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('template', type=Path)
    parser.add_argument('--cli', default=os.environ.get('POML_CLI'))
    parser.add_argument('--context-file', type=Path, action='append', default=[])
    args = parser.parse_args()
    if not args.cli or not Path(args.cli).is_file():
        parser.error('Set --cli or POML_CLI to the installed Microsoft POML JavaScript CLI')
    if not args.template.is_file():
        parser.error(f'Missing template: {args.template}')
    try:
        cases = [(str(p), json.loads(p.read_text(encoding='utf-8'))) for p in args.context_file]
        if not cases:
            cases = [('empty context', {})]
        if not all(isinstance(data, dict) for _, data in cases):
            parser.error('Every context must be a JSON object')
        with tempfile.TemporaryDirectory(prefix='praxis-poml-validate-') as folder:
            for label, data in cases:
                context = Path(folder) / 'context.json'
                context.write_text(json.dumps(data, ensure_ascii=False), encoding='utf-8')
                result = subprocess.run(
                    ['node', str(Path(args.cli).resolve()), '--file', str(args.template.resolve()),
                     '--context-file', str(context), '--strict', '--speakerMode=false'],
                    text=True, capture_output=True, timeout=30, cwd=folder)
                if result.returncode:
                    raise RuntimeError(f'{label}: POML validation failed (exit {result.returncode})\n{result.stderr[:1500]}')
                output = json.loads(result.stdout).get('messages')
                if not isinstance(output, str) or not output.strip():
                    raise RuntimeError(f'{label}: POML returned empty/non-text output')
                print(f'PASS: {label}\n{output}')
    except (OSError, ValueError, subprocess.TimeoutExpired, RuntimeError) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
