#!/usr/bin/env python3
"""20 natural tasks, paired real Praxis runs; state selection != answer quality.

Needs a compiled library test executable, real POML and an already running model.
Never starts a GPU instance. Rubrics stay in the observer, outside model context.
"""
import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time
import urllib.request


def load(path, default=None):
    return json.loads(path.read_text()) if path.exists() else default


def receipt_text(receipt):
    text = receipt.get('content', '')
    try:
        value = json.loads(text)
        if isinstance(value, dict) and isinstance(value.get('text'), str):
            return value['text']
    except (ValueError, TypeError):
        pass
    return text


def analyse_run(root):
    root = Path(root)
    fixture = load(root / 'fixture.json')
    task, arm = fixture['task'], fixture['arm']
    result = load(root / 'result.json', {})
    http = result.get('http_result') or {}
    final_state = (result.get('final_context') or {}).get('active_state')
    receipts = {}
    for message in result.get('history', []):
        if message.get('role') == 'tool':
            receipts.setdefault(message.get('tool_call_id'), []).append(message)
    state = task['initial_state']
    transitions, deterministic, no_ops, observed, attempts = [], [], [], [], []
    read_seen = False
    appropriate_after_evidence = False
    prompt_tokens = completion_tokens = 0
    calls = sorted(root.glob('call-*.request.json'))
    for request_path in calls:
        number = int(request_path.name.split('.')[0].split('-')[1])
        request = load(request_path)
        actual = (request.get('observed_state') or {}).get('active_state')
        if actual is not None:
            observed.append(actual)
            if actual != state:
                deterministic.append({'from': state, 'to': actual, 'before_call': number})
                state = actual
            if read_seen and actual in task['acceptable_states']:
                appropriate_after_evidence = True
        response = (load(root / f'call-{number:02}.response.json', {}).get('response') or {})
        usage = response.get('usage') or {}
        prompt_tokens += usage.get('prompt_tokens', 0)
        completion_tokens += usage.get('completion_tokens', 0)
        for call in response.get('tool_calls') or []:
            name = call.get('function', {}).get('name', '')
            attempts.append(name)
            matches = receipts.get(call.get('id'), [])
            if len(matches) != 1:
                continue  # generation/stream fragments are not execution
            text = receipt_text(matches[0])
            if name == 'read_file' and not text.startswith('Error'):
                read_seen = True
            if name != 'set_context' or not text.startswith("Context key 'active_state' set"):
                continue
            try:
                args = json.loads(call['function']['arguments'])
            except (ValueError, KeyError):
                continue
            if args.get('key') != 'active_state' or not isinstance(args.get('value'), str):
                continue
            change = {'from': state, 'to': args['value'], 'call': number,
                      'tool_call_id': call['id'], 'after_evidence_read': read_seen}
            (no_ops if state == args['value'] else transitions).append(change)
            state = args['value']
    if final_state is not None and state != final_state:
        deterministic.append({'from': state, 'to': final_state, 'after_call': 'last'})
    answer = http.get('response') or ''
    answer = answer if isinstance(answer, str) else ''
    checks = [{'pattern': pattern, 'passed': bool(re.search(pattern, answer, re.I | re.S))}
              for pattern in task['checks']]
    complete = http.get('success') is True
    evidence_ok = 'fixture' not in task or read_seen
    changed_or_attempted_no_op = transitions + no_ops
    policy = (not changed_or_attempted_no_op if arm == 'fixed' else
              len(changed_or_attempted_no_op) <= 1 and all(c['call'] == 0 for c in changed_or_attempted_no_op)
              if arm == 'entry' else True)
    return dict(id=task['id'], title=task.get('title', ''), arm=arm, completed=complete,
                task_rubric_ok=complete and evidence_ok and all(c['passed'] for c in checks),
                checks=checks, evidence_read=read_seen, evidence_requirement_met=evidence_ok,
                appropriate_state_used=any(s in task['acceptable_states'] for s in observed),
                appropriate_state_after_evidence=appropriate_after_evidence,
                final_state=final_state, final_state_matches=final_state in task['acceptable_states'],
                observed_states=observed, model_transitions=transitions,
                deterministic_transitions=deterministic, redundant_state_writes=no_ops,
                policy_followed=policy, tool_attempts=dict(Counter(attempts)),
                blocked_batches=len(list(root.glob('call-*.guard.json'))),
                model_calls=len(calls), prompt_tokens=prompt_tokens, completion_tokens=completion_tokens,
                elapsed_s=round(result.get('elapsed_ms', 0)/1000, 3),
                error=http.get('error') or result.get('transport_error'), artifacts=str(root))


def compare_arms(rows, baseline, candidate):
    by_key = {(r['id'], r['arm']): r for r in rows}
    pairs = [(by_key[(i, baseline)], by_key[(i, candidate)])
             for i in sorted({r['id'] for r in rows})
             if (i, baseline) in by_key and (i, candidate) in by_key]
    return dict(baseline=baseline, candidate=candidate, paired_tasks=len(pairs),
                rubric_improved_tasks=sum(not a['task_rubric_ok'] and b['task_rubric_ok'] for a, b in pairs),
                rubric_regressed_tasks=sum(a['task_rubric_ok'] and not b['task_rubric_ok'] for a, b in pairs),
                rubric_equal_tasks=sum(a['task_rubric_ok'] == b['task_rubric_ok'] for a, b in pairs),
                appropriate_state_delta=sum(int(b['appropriate_state_used'])-int(a['appropriate_state_used']) for a,b in pairs),
                prompt_token_delta=sum(b['prompt_tokens']-a['prompt_tokens'] for a,b in pairs),
                completion_token_delta=sum(b['completion_tokens']-a['completion_tokens'] for a,b in pairs),
                elapsed_s_delta=round(sum(b['elapsed_s']-a['elapsed_s'] for a,b in pairs), 3))


def write_reports(output, rows, requested):
    arms = sorted({r['arm'] for r in rows})
    totals = {}
    for arm in arms:
        selected = [r for r in rows if r['arm'] == arm]
        totals[arm] = dict(runs=len(selected), completed=sum(r['completed'] for r in selected),
                          rubric_passed=sum(r['task_rubric_ok'] for r in selected),
                          appropriate_state_used=sum(r['appropriate_state_used'] for r in selected),
                          policy_followed=sum(r['policy_followed'] for r in selected),
                          model_transitions=sum(len(r['model_transitions']) for r in selected),
                          post_evidence_transitions=sum(c['after_evidence_read'] for r in selected for c in r['model_transitions']),
                          deterministic_transitions=sum(len(r['deterministic_transitions']) for r in selected),
                          redundant_state_writes=sum(len(r['redundant_state_writes']) for r in selected),
                          prompt_tokens=sum(r['prompt_tokens'] for r in selected),
                          completion_tokens=sum(r['completion_tokens'] for r in selected),
                          median_elapsed_s=round(statistics.median(r['elapsed_s'] for r in selected),3))
    comparisons = [compare_arms(rows, baseline, 'continuous') for baseline in ['fixed','entry'] if baseline in arms and 'continuous' in arms]
    summary = dict(requested_runs=requested, completed_runs=len(rows), arms=totals, comparisons=comparisons,
                   caveats=['Rubric checks are automated proxies, not proof of full semantic correctness.',
                            'A higher state-switch count is not itself a benefit.',
                            'Runtime initialization/non-model changes are counted separately.',
                            'Latency includes queueing; concurrent GPU load is not controlled.',
                            'Twenty distinct tasks with three arms means sixty complete task runs, not sixty distinct tasks.'])
    for name, data in [('rows.json', rows), ('summary.json', summary)]:
        temporary = output / (name + '.tmp')
        temporary.write_text(json.dumps(data, ensure_ascii=False, indent=2))
        temporary.replace(output / name)
    md = ['# 20-task real-state experiment', '',
          '| Arm | Runs | Completed | Rubric OK | Appropriate state used | Policy OK | Model transitions | After file evidence |',
          '|---|---:|---:|---:|---:|---:|---:|---:|']
    for arm, s in totals.items():
        md.append(f"| {arm} | {s['runs']} | {s['completed']} | {s['rubric_passed']} | {s['appropriate_state_used']} | {s['policy_followed']} | {s['model_transitions']} | {s['post_evidence_transitions']} |")
    md += ['', '## Paired comparisons', '', '```json', json.dumps(comparisons, indent=2), '```', '',
           '## Per-task traces', '', '| Task | Arm | Actual model-request states | Rubric OK |', '|---|---|---|---|']
    for row in rows:
        md.append(f"| {row['id']} | {row['arm']} | {' → '.join(row['observed_states'])} | {row['task_rubric_ok']} |")
    md += ['', '## Limits', *('- '+s for s in summary['caveats']), '']
    (output / 'REPORT.md').write_text('\n'.join(md))
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-binary', required=True, type=Path)
    parser.add_argument('--runtime-root', type=Path, default=Path.cwd())
    parser.add_argument('--url', required=True)
    parser.add_argument('--model', required=True)
    parser.add_argument('--poml-cli', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--cases', default=','.join(f'{i:02}' for i in range(1,21)))
    parser.add_argument('--arms', default='fixed,entry,continuous')
    parser.add_argument('--allow-paid-live-inference', action='store_true')
    args = parser.parse_args()
    if not args.allow_paid_live_inference:
        parser.error('Explicit --allow-paid-live-inference required; existing GPU only')
    cases, arms = args.cases.split(','), args.arms.split(',')
    if not cases or any(c not in {f'{i:02}' for i in range(1,21)} for c in cases) or len(set(cases)) != len(cases):
        parser.error('Select unique case IDs 01..20')
    if not arms or any(a not in {'fixed','entry','continuous'} for a in arms) or len(set(arms)) != len(arms):
        parser.error('Select unique arms: fixed,entry,continuous')
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    binary, cwd, cli = args.test_binary.resolve(), args.runtime_root.resolve(), args.poml_cli.resolve()
    rows = []
    requested = len(cases)*len(arms)
    write_reports(args.output, rows, requested)
    env = os.environ.copy()
    env.pop('GPU_ROUTER_URL', None)
    env.update(POML_CLI=str(cli), PRAXIS_LIVE_URL=args.url, PRAXIS_LIVE_MODEL=args.model)
    for index, case in enumerate(cases):
        # Rotate arm order to reduce simple warm-cache/order bias. No claim of
        # controlled latency: other clients may still share the GPU.
        shift = index % len(arms)
        for arm in arms[shift:]+arms[:shift]:
            with urllib.request.urlopen(args.url.rstrip('/')+'/health', timeout=15) as health:
                if health.status != 200:
                    raise RuntimeError('Model not healthy; refusing to wake or rent')
            out = args.output / f'{case}-{arm}'
            task_env = dict(env, PRAXIS_LIVE_ARTIFACTS=str(out), PRAXIS_STATE_CASE=case, PRAXIS_STATE_ARM=arm)
            start = time.monotonic()
            with (args.output / f'{case}-{arm}.log').open('w') as log:
                try:
                    exit_status = subprocess.run([str(binary), 'local_model_live_state_task', '--ignored', '--nocapture', '--test-threads=1'],
                                                 cwd=cwd, env=task_env, stdout=log, stderr=subprocess.STDOUT, timeout=420).returncode
                except subprocess.TimeoutExpired:
                    exit_status = 124
            if not (out / 'fixture.json').exists():
                raise RuntimeError(f'Fixture failed before inference: {case}/{arm}; see its log')
            row = analyse_run(out)
            row.update(exit_status=exit_status, wall_elapsed_s=round(time.monotonic()-start,3))
            rows.append(row)
            write_reports(args.output, rows, requested)
            print(f"{len(rows)}/{requested} {case}/{arm}: completed={row['completed']} rubric={row['task_rubric_ok']} model_switches={len(row['model_transitions'])} state={row['final_state']}", flush=True)


if __name__ == '__main__':
    main()
