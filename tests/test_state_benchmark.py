"""Scoring regressions: a claimed persona is not an executed state transition."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
from bench_state_machine import analyse_run, compare_arms


class StateScoringTests(unittest.TestCase):
    def fixture(self, root, *, call=False, receipt=False, observed='standard', final='standard', arm='continuous'):
        task = {'id': '01', 'initial_state': 'standard', 'acceptable_states': ['debugger'], 'checks': ['cause']}
        def save(name, value):
            (root / name).write_text(json.dumps(value))
        save('fixture.json', {'task': task, 'arm': arm})
        tc = {'id': 'change', 'function': {'name': 'set_context', 'arguments': '{"key":"active_state","value":"debugger"}'}}
        save('call-00.request.json', {'observed_state': {'active_state': observed}})
        save('call-00.response.json', {'response': {'content': 'I am now a debugger. cause', 'tool_calls': [tc] if call else None}})
        history = [{'role': 'tool', 'tool_call_id': 'change', 'tool_name': 'set_context', 'content': "Context key 'active_state' set"}] if receipt else []
        save('result.json', {'http_result': {'success': True, 'response': 'cause'}, 'final_context': {'active_state': final}, 'history': history})

    def test_prose_and_attempt_without_receipt_are_not_transitions(self):
        for call in [False, True]:
            with tempfile.TemporaryDirectory() as d:
                root = Path(d)
                self.fixture(root, call=call)
                row = analyse_run(root)
                self.assertEqual(row['model_transitions'], [])
                self.assertTrue(row['task_rubric_ok'])
                self.assertFalse(row['appropriate_state_used'])

    def test_receipt_and_observed_context_confirm_a_real_transition(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.fixture(root, call=True, receipt=True, final='debugger')
            (root / 'call-01.request.json').write_text(json.dumps({'observed_state': {'active_state': 'debugger'}}))
            (root / 'call-01.response.json').write_text(json.dumps({'response': {'content': 'cause'}}))
            row = analyse_run(root)
            self.assertEqual(len(row['model_transitions']), 1)
            self.assertEqual(row['deterministic_transitions'], [])
            self.assertTrue(row['appropriate_state_used'])
            self.assertTrue(row['policy_followed'])

    def test_automatic_routing_is_separate_from_model_selection(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.fixture(root, observed='debugger', final='debugger')
            row = analyse_run(root)
            self.assertEqual(row['model_transitions'], [])
            self.assertEqual(len(row['deterministic_transitions']), 1)

    def test_error_receipt_does_not_count(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.fixture(root, call=True)
            result = json.loads((root / 'result.json').read_text())
            result['history'] = [{'role': 'tool', 'tool_call_id': 'change', 'content': 'Error: undefined state'}]
            (root / 'result.json').write_text(json.dumps(result))
            self.assertEqual(analyse_run(root)['model_transitions'], [])

    def test_more_switches_are_not_automatically_a_benefit(self):
        rows = [{'id': '01', 'arm': arm, 'task_rubric_ok': True, 'appropriate_state_used': arm == 'continuous',
                 'prompt_tokens': 100 if arm == 'fixed' else 200, 'completion_tokens': 10, 'elapsed_s': 1.0}
                for arm in ['fixed', 'continuous']]
        result = compare_arms(rows, 'fixed', 'continuous')
        self.assertEqual(result['rubric_improved_tasks'], 0)
        self.assertEqual(result['rubric_equal_tasks'], 1)
        self.assertEqual(result['prompt_token_delta'], 100)


if __name__ == '__main__':
    unittest.main()
