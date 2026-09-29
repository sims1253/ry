"""Tests for the compact CI performance report."""

import json
import tempfile
import unittest
from pathlib import Path

from performance.collect import collect


class PerformanceReportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.estimate = self.root / 'target/criterion/branch/128/new'
        self.estimate.mkdir(parents=True)
        (self.estimate / 'benchmark.json').write_text(json.dumps({'full_id': 'branch/128'}))
        self.write_estimate(42)
        for path in (
            'target/release/ry',
            'editors/code/dist/extension.js',
            'editors/code/ry.vsix',
            'editors/zed/target/wasm32-wasip2/release/zed_ry.wasm',
        ):
            artifact = self.root / path
            artifact.parent.mkdir(parents=True, exist_ok=True)
            artifact.write_bytes(b'12345')

    def write_estimate(self, value):
        (self.estimate / 'estimates.json').write_text(json.dumps({
            'mean': {
                'point_estimate': value,
                'confidence_interval': {'lower_bound': 40, 'upper_bound': 44},
            },
        }))

    def write_replay(self):
        path = self.root / 'server-replay.json'
        report = {
            'schema_version': 1,
            'correctness': 'passed',
            'workload_sha256': 'a' * 64,
            'binary_sha256': 'b' * 64,
            'binary_version': 'ry 0.11.0',
            'rustc': 'rustc 1.96.1',
            'rayon_threads': '2',
            'sample_counts': {'startup': 5, 'warm_per_scenario': 30},
            'startup_ns': [n * 1_000_000 for n in (1, 3, 5, 7, 9)],
            'warm': {},
        }
        for scenario, target in {
            'local-clean': 'R/local.R',
            'cross-file-caller': 'R/caller.R',
            'unrelated-file': 'R/unrelated.R',
        }.items():
            report['warm'][scenario] = [
                {
                    'duration_ns': n * 1_000_000,
                    'snapshot_sha256': 'c' * 64,
                    'completion': target,
                    'edited_version': n + 1,
                    'completion_version': 1,
                }
                for n in range(1, 31)
            ]
        path.write_text(json.dumps(report))
        return path, report

    def test_collects_grouped_estimates_and_sizes(self):
        # Old Criterion baselines must not become additional measurements.
        old = self.estimate.parent / 'base'
        old.mkdir()
        (old / 'estimates.json').write_text('{}')
        result = collect(self.root)
        self.assertEqual(len(result), 5)
        self.assertEqual(result[0]['name'], 'core/branch/128')
        self.assertEqual(result[0]['value'], 42)
        self.assertEqual(result[0]['unit'], 'ns')
        self.assertTrue(all(row['value'] == 5 for row in result[1:]))
        self.assertTrue(all(row['unit'] == 'bytes' for row in result[1:]))

    def test_missing_benchmark_fails(self):
        (self.estimate / 'estimates.json').unlink()
        with self.assertRaisesRegex(ValueError, 'No Criterion estimates'):
            collect(self.root)

    def test_invalid_estimate_fails(self):
        for value in (0, -1, float('nan'), float('inf')):
            with self.subTest(value=value):
                self.write_estimate(value)
                with self.assertRaisesRegex(ValueError, 'Invalid estimate'):
                    collect(self.root)

    def test_activation_metrics_are_included_and_validated(self):
        activation = self.root / 'activation.json'
        rows = [
            {'name': 'vscode/activation', 'unit': 'ms', 'value': 10},
            {'name': 'vscode/activation-to-first-diagnostic', 'unit': 'ms', 'value': 200},
        ]
        activation.write_text(json.dumps(rows))
        self.assertEqual(collect(self.root, activation)[-2:], rows)
        rows[1]['name'] = rows[0]['name']
        activation.write_text(json.dumps(rows))
        with self.assertRaisesRegex(ValueError, 'Missing or duplicate'):
            collect(self.root, activation)

    def test_missing_extension_fails(self):
        (self.root / 'editors/code/ry.vsix').unlink()
        with self.assertRaises(FileNotFoundError):
            collect(self.root)

    def test_server_replay_uses_raw_warm_samples_for_median_and_nearest_rank_p95(self):
        path, _ = self.write_replay()
        rows = collect(self.root, server_replay=path)
        by_name = {row['name']: row['value'] for row in rows}
        self.assertEqual(by_name['server/startup-median'], 5)
        self.assertEqual(by_name['server/local-clean/median'], 15.5)
        self.assertEqual(by_name['server/local-clean/p95'], 29)
        self.assertNotIn('server/startup-p95', by_name)
        self.assertEqual(len(rows), 12)

    def test_server_replay_rejects_failed_or_small_measurements(self):
        path, report = self.write_replay()
        report['correctness'] = 'failed'
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'correctness'):
            collect(self.root, server_replay=path)
        report['correctness'] = 'passed'
        report['warm']['local-clean'].pop()
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'exactly 30'):
            collect(self.root, server_replay=path)

    def test_server_replay_requires_snapshot_and_binary_identity(self):
        path, report = self.write_replay()
        report['binary_sha256'] = ''
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'binary_sha256'):
            collect(self.root, server_replay=path)
        report['binary_sha256'] = 'b' * 64
        report['warm']['cross-file-caller'][0]['completion'] = 'R/helper.R'
        path.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'completion snapshot'):
            collect(self.root, server_replay=path)


if __name__ == '__main__':
    unittest.main()
