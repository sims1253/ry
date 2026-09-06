"""Collect Criterion estimates and extension sizes without retaining binaries."""

import argparse
import json
import math
from pathlib import Path


def collect(root, activation=None):
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
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--activation', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(collect(args.root, args.activation), indent=2) + '\n')
