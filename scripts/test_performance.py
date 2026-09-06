"""Tests for the compact CI performance report."""

import json
from pathlib import Path
import tempfile
import unittest

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


if __name__ == '__main__':
    unittest.main()
