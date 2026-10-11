"""Collect Criterion estimates and extension sizes without retaining binaries."""

import argparse
import json
import math
from pathlib import Path
from statistics import median


def replay_rows(path):
    # crates/ry-lsp/tests/server_replay.rs validates every sample before it
    # writes this report; only summarize it here.
    report = json.loads(path.read_text())
    rows = [{
        'name': 'server/startup-median', 'unit': 'ms',
        'value': median(report['startup_ns']) / 1_000_000,
    }]
    for scenario, samples in report['warm'].items():
        values = sorted(sample['duration_ns'] for sample in samples)
        # Nearest-rank p95: ceiling(0.95 * n), one-indexed. The five startup
        # observations are too few for a tail estimate.
        p95 = values[math.ceil(0.95 * len(values)) - 1]
        rows.extend([
            {'name': f'server/{scenario}/median', 'unit': 'ms', 'value': median(values) / 1_000_000},
            {'name': f'server/{scenario}/p95', 'unit': 'ms', 'value': p95 / 1_000_000},
        ])
    return rows


def collect(root, activation=None, server_replay=None):
    results = []
    for estimate in sorted((root / 'target/criterion').glob('**/new/estimates.json')):
        benchmark = json.loads((estimate.parent / 'benchmark.json').read_text())
        mean = json.loads(estimate.read_text())['mean']
        value = mean['point_estimate']
        if not math.isfinite(value) or value <= 0:
            raise ValueError(f'Invalid estimate: {estimate}')
        interval = mean['confidence_interval']
        results.append({
            'name': f"core/{benchmark['full_id']}",
            'unit': 'ns',
            'value': value,
            'range': f"{interval['lower_bound']:.2f}–{interval['upper_bound']:.2f}",
        })
    if not results:
        raise ValueError('No Criterion estimates found; run the performance benchmark first')
    for name, path in [
        ('cli/executable', 'target/release/ry'),
        ('vscode/javascript', 'editors/code/dist/extension.js'),
        ('vscode/vsix-without-server', 'editors/code/ry.vsix'),
        ('zed/wasm', 'editors/zed/target/wasm32-wasip2/release/zed_ry.wasm'),
    ]:
        size = (root / path).stat().st_size
        if size == 0:
            raise ValueError(f'Empty artifact: {path}')
        results.append({'name': name, 'unit': 'bytes', 'value': size})
    if activation is not None:
        metrics = json.loads(activation.read_text())
        expected = {'vscode/activation', 'vscode/activation-to-first-diagnostic'}
        if len(metrics) != 2 or {row['name'] for row in metrics} != expected:
            raise ValueError('Missing or duplicate activation metrics')
        for row in metrics:
            if row['unit'] != 'ms' or not math.isfinite(row['value']) or row['value'] <= 0:
                raise ValueError('Invalid activation metric')
        results.extend(metrics)
    if server_replay is not None:
        results.extend(replay_rows(server_replay))
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--activation', type=Path, required=True)
    parser.add_argument('--server-replay', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(collect(args.root, args.activation, args.server_replay), indent=2) + '\n')
