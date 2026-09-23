#!/usr/bin/env python3
"""Opt-in paid LIVE Praxis pipeline benchmark. No instance creation or budget changes.
Runs the ignored real-gateway fixture in fresh processes/DBs; not a provider-only bench.
500 means 500 complete tasks, NOT 500 distinct tools or 500 unit assertions.
Credentials, if needed, are supplied only via PRAXIS_LIVE_API_KEY in the environment.
"""
import argparse
import collections
import json
import os
from pathlib import Path
import subprocess
import time
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--test-binary', required=True, type=Path)
parser.add_argument('--url', required=True)
parser.add_argument('--model', required=True)
parser.add_argument('--poml-cli', required=True, type=Path)
parser.add_argument('--output', required=True, type=Path)
parser.add_argument('--runtime-root', type=Path, default=Path.cwd(), help='Frozen templates/contexts/skills working directory')
parser.add_argument('--count', type=int, default=500)
parser.add_argument('--allow-paid-live-inference', action='store_true', required=True)
args = parser.parse_args()
if not 1 <= args.count <= 500:
    parser.error('count must be 1..500')
args.output.mkdir(mode=0o700, parents=True, exist_ok=False)
env = os.environ.copy()
env.pop('GPU_ROUTER_URL', None)  # no implicit wake hook
for key in ['PRAXIS_LIVE_FILE_LINES', 'PRAXIS_LIVE_TURNS']:
    env.pop(key, None)
env.update(POML_CLI=str(args.poml_cli.resolve()), PRAXIS_LIVE_URL=args.url, PRAXIS_LIVE_MODEL=args.model)
headers = {}
if env.get('PRAXIS_LIVE_API_KEY'):
    headers['Authorization'] = 'Bearer ' + env['PRAXIS_LIVE_API_KEY']
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

def get(path):
    with opener.open(urllib.request.Request(args.url.rstrip('/') + path, headers=headers), timeout=15) as response:
        return json.load(response)

props = get('/props')
if props.get('model_alias') != args.model:
    raise SystemExit('Loaded model does not match the requested model; no benchmark started')
(args.output / 'props.json').write_text(json.dumps(props, indent=2))
rows = []
started = time.time()
consecutive_infrastructure_failures = 0
# Safety-limited coverage, intentionally explicit. Expand only with audited fixtures.
cases = [('read_file', 0, 1), ('discovery', 0, 1), ('read_tail', 5000, 1), ('discovery', 0, 6)]
for index in range(args.count):
    try:
        health = get('/health')
        if health.get('status') != 'ok':
            raise RuntimeError('model is not ready')
    except Exception:
        (args.output / 'stopped.txt').write_text('Backend not healthy; stopped without waking/renting any instance.\n')
        break
    case, lines, turns = cases[index % len(cases)]
    thinking = 'off' if (index // len(cases)) % 2 == 0 else 'low'
    out = args.output / f'{index:04d}-{case}-{thinking}'
    task_env = dict(env, PRAXIS_LIVE_ARTIFACTS=str(out.resolve()), PRAXIS_LIVE_CASE=case,
                    PRAXIS_LIVE_FILE_LINES=str(lines), PRAXIS_LIVE_TURNS=str(turns), PRAXIS_LIVE_THINKING=thinking)
    start = time.time()
    with (args.output / f'{index:04d}.log').open('w') as log:
        try:
            status = subprocess.run([str(args.test_binary.resolve()), 'local_model_live_tool_calling',
                                     '--ignored', '--nocapture', '--test-threads=1'],
                                    env=task_env, cwd=args.runtime_root.resolve(), stdout=log, stderr=subprocess.STDOUT, timeout=420).returncode
        except subprocess.TimeoutExpired:
            status = 124
    calls = []
    for file in sorted(out.glob('call-*.response.json')):
        data = json.loads(file.read_text())
        response = data.get('response') or {}
        calls.append({'finish_reason':response.get('finish_reason'), 'usage':response.get('usage'),
                      'elapsed_ms':data.get('elapsed_ms'), 'error':data.get('error'),
                      'tools': [c['function']['name'] for c in response.get('tool_calls') or []]})
    history = []
    if (out / 'result.json').exists():
        history = json.loads((out / 'result.json').read_text()).get('history', [])
    receipts = {m.get('tool_call_id'): m for m in history if m.get('role') == 'tool'}
    attempted = [tc for f in sorted(out.glob('call-*.response.json'))
                 for tc in (json.loads(f.read_text()).get('response') or {}).get('tool_calls') or []]
    successful = 0
    valid_json_args = 0
    for tc in attempted:
        try:
            valid_json_args += isinstance(json.loads(tc['function']['arguments']), dict)
        except (ValueError, KeyError):
            pass
        receipt = receipts.get(tc.get('id'))
        if receipt and not receipt.get('content', '').startswith('Error'):
            successful += 1
    row = dict(index=index, case=case, thinking=thinking, agent_turn_limit=turns,
               passed=status == 0, exit_status=status, elapsed_s=round(time.time()-start, 3), calls=calls,
               attempted_tool_calls=len(attempted), valid_json_arguments=valid_json_args,
               successful_receipts=successful, failed_or_blocked_calls=len(attempted)-successful,
               extra_calls_above_fixture_minimum=max(0,len(attempted)-(2 if case=='discovery' else 1)))
    rows.append(row)
    (args.output / 'tasks.json').write_text(json.dumps(rows, indent=2))
    summary = {'requested_tasks':args.count, 'completed_tasks':len(rows),
               'passed_tasks':sum(r['passed'] for r in rows),
               'failed_tasks':sum(not r['passed'] for r in rows),
               'elapsed_s':round(time.time()-started, 1),
               'attempted_tool_calls':sum(r['attempted_tool_calls'] for r in rows),
               'successful_receipts':sum(r['successful_receipts'] for r in rows),
               'failed_or_blocked_calls':sum(r['failed_or_blocked_calls'] for r in rows),
               'tool_call_attempts':dict(collections.Counter(t for r in rows for c in r['calls'] for t in c['tools'])),
               'coverage':'read_file, search_tools, memory_profile_list, read_tail via _output; off/low; chat/agent; isolated data'}
    (args.output / 'summary.json').write_text(json.dumps(summary, indent=2))
    print(json.dumps({k:summary[k] for k in ['completed_tasks','passed_tasks','failed_tasks','elapsed_s']}), flush=True)
    infrastructure = status == 124 or any('HTTP 503' in (c['error'] or '') for c in calls)
    consecutive_infrastructure_failures = consecutive_infrastructure_failures + 1 if infrastructure else 0
    if consecutive_infrastructure_failures >= 3:
        (args.output / 'stopped.txt').write_text('Stopped after 3 consecutive infrastructure failures; no retries or new instances.\n')
        break
