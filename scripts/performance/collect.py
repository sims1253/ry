"""Collect Criterion estimates and extension sizes without retaining binaries."""

import argparse
import json
import math
from pathlib import Path
from statistics import median


def replay_rows(path):
    report = json.loads(path.read_text())
    if not isinstance(report, dict) or report.get('schema_version') != 1 or report.get('correctness') != 'passed':
        raise ValueError('Server replay is missing validated schema/correctness')
    for key in ('workload_sha256', 'binary_sha256'):
        digest = report.get(key)
        if not isinstance(digest, str) or len(digest) != 64 or any(
            character not in '0123456789abcdef' for character in digest
        ):
            raise ValueError(f'Invalid server replay {key}')
    if any(not isinstance(report.get(key), str) or not report[key].strip()
           for key in ('binary_version', 'rustc', 'rayon_threads')):
        raise ValueError('Server replay lacks binary or toolchain identity')

    counts = report.get('sample_counts')
    if not isinstance(counts, dict) or set(counts) != {'startup', 'warm_per_scenario'}:
        raise ValueError('Server replay lacks sample counts')
    if any(type(value) is not int or value < minimum for value, minimum in (
        (counts['startup'], 5), (counts['warm_per_scenario'], 30)
    )):
        raise ValueError('Server replay has too few samples')

    def durations(samples, minimum, name):
        if not isinstance(samples, list) or len(samples) != minimum:
            raise ValueError(f'{name} needs exactly {minimum} reported samples')
        if name != 'startup' and any(not isinstance(sample, dict) for sample in samples):
            raise ValueError(f'{name} has malformed samples')
        values = [
            sample if name == 'startup' else sample.get('duration_ns')
            for sample in samples
        ]
        if any(type(value) is not int or value <= 0 for value in values):
            raise ValueError(f'{name} has an invalid duration')
        return values

    startup = durations(report.get('startup_ns'), counts['startup'], 'startup')
    rows = [{
        'name': 'server/startup-median', 'unit': 'ms',
        'value': median(startup) / 1_000_000,
    }]
    targets = {
        'local-clean': 'R/local.R',
        'cross-file-caller': 'R/caller.R',
        'unrelated-file': 'R/unrelated.R',
    }
    warm = report.get('warm')
    if not isinstance(warm, dict) or set(warm) != set(targets):
        raise ValueError('Server replay has missing or extra scenarios')
    for scenario, target in targets.items():
        samples = warm[scenario]
        values = durations(samples, counts['warm_per_scenario'], scenario)
        for sample in samples:
            digest = sample.get('snapshot_sha256')
            if sample.get('completion') != target or not isinstance(digest, str) or len(digest) != 64:
                raise ValueError(f'{scenario} lacks a completion snapshot')
            if any(character not in '0123456789abcdef' for character in digest):
                raise ValueError(f'{scenario} has an invalid snapshot hash')
            if any(type(sample.get(key)) is not int or sample[key] < 1
                   for key in ('edited_version', 'completion_version')):
                raise ValueError(f'{scenario} lacks document versions')
        ordered = sorted(values)
        # Nearest-rank p95: ceiling(0.95 * n), one-indexed. Never calculate a
        # tail percentile from the five fresh-process startup observations.
        p95 = ordered[math.ceil(0.95 * len(ordered)) - 1]
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
