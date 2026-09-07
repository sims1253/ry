#!/usr/bin/env python3
"""Compare cloned baseline and journal candidate from separate worktrees."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--baseline-repo', type=Path, required=True)
parser.add_argument('--out', type=Path, required=True)
parser.add_argument('--tools', nargs='+', choices=['callgrind', 'dhat'], default=['callgrind', 'dhat'])
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
baseline = args.baseline_repo.resolve()
if baseline == repo:
    parser.error('baseline needs its own worktree and target directory')
args.out.mkdir(parents=True, exist_ok=False)
out = args.out.resolve()
for kind in ['sparse', 'dense', 'alternating']:
    directory = out / kind
    directory.mkdir()
    source = 'f <- function(flag) {\n' + ''.join(f'x{i} <- {i}L\n' for i in range(1024))
    for depth in range(24):
        source += 'if (flag) {\n'
        value = '"changed"' if kind != 'alternating' or depth % 2 == 0 else '1L'
        names = [depth] if kind == 'sparse' else range(1024)
        source += ''.join(f'x{i} <- {value}\n' for i in names)
    (directory / 'branches.R').write_text(source + '}\n' * 24 + 'x0\n}\nf(TRUE)\n')
corpus = out / 'corpus'
corpus.mkdir()
for source in (repo / 'crates/ry-checker/testdata').glob('*.R'):
    shutil.copyfile(source, corpus / source.name)
env = dict(os.environ, RAYON_NUM_THREADS='1', RY_NO_INSTALLED_LIBRARIES='1')
binaries = {}
locks = []
for enabled, checkout in [(False, baseline), (True, repo)]:
    harness = out / ('harness-journal' if enabled else 'harness-clone')
    (harness / 'src').mkdir(parents=True)
    shutil.copyfile(Path(__file__).with_name('main.rs'), harness / 'src/main.rs')
    (harness / 'Cargo.toml').write_text(
        '[package]\nname="ry-scope-probe"\nversion="0.1.0"\nedition="2024"\n'
        '[workspace]\n[dependencies]\n'
        + '\n'.join(f'{name}={{path={json.dumps(str(checkout / "crates" / name))}}}' for name in ['ry-checker', 'ry-core'])
        + '\n[profile.release]\ndebug=0\n')
    # Seed exact repository dependency versions; Cargo only adds the probe and
    # drops unrelated workspace packages. Check shared versions below.
    shutil.copyfile(checkout / 'Cargo.lock', harness / 'Cargo.lock')
    target = checkout / 'profile-target'
    subprocess.run(['cargo', 'build', '--release', '--offline', '--manifest-path', str(harness / 'Cargo.toml')], env=dict(env, CARGO_TARGET_DIR=str(target)), check=True)
    binary = out / ('ry-scope-probe-journal' if enabled else 'ry-scope-probe-clone')
    shutil.copy2(target / 'release/ry-scope-probe', binary)
    binaries[enabled] = binary
    packages = []
    for block in (harness / 'Cargo.lock').read_text().split('[[package]]')[1:]:
        packages.append(dict(re.findall(r'^(name|version|source) = "([^"]*)"$', block, re.MULTILINE)))
    versions = {}
    for package in packages:
        versions.setdefault(package['name'], set()).add((package['version'], package.get('source')))
    locks.append(versions)
for key in locks[0].keys() & locks[1].keys():
    assert locks[0][key] == locks[1][key], f'dependency version mismatch: {key}'
results = {
    'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip(),
    'baseline_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=baseline, text=True).strip(),
    'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
    'valgrind': subprocess.check_output(['valgrind', '--version'], text=True).strip(),
    'binary_sha256': {str(enabled): hashlib.sha256(binary.read_bytes()).hexdigest() for enabled, binary in binaries.items()},
    'settings': {'rayon_threads': 1, 'no_installed_libraries': True},
    'inputs': {}, 'measurements': [],
}
for workload in ['sparse', 'dense', 'alternating', 'corpus']:
    directory = out / workload
    digest = hashlib.sha256()
    for source in sorted(directory.glob('*.R')):
        digest.update(source.name.encode() + b'\0' + source.read_bytes())
    results['inputs'][workload] = {'sha256': digest.hexdigest(), 'files': len(list(directory.glob('*.R')))}
    for tool in args.tools:
        for enabled, binary in binaries.items():
            stem = out / f'{workload}-{tool}-{int(enabled)}'
            flag = f'--callgrind-out-file={stem}.profile' if tool == 'callgrind' else f'--dhat-out-file={stem}.profile'
            with stem.with_suffix('.log').open('w') as log:
                subprocess.run(['valgrind', '--command-line-only=yes', f'--tool={tool}', flag, str(binary), str(directory), str(stem.with_suffix('.diagnostics'))], env=env, stdout=log, stderr=log, check=True)
            text = stem.with_suffix('.log').read_text()
            row = {'workload': workload, 'tool': tool, 'journal': enabled}
            if tool == 'callgrind':
                row['instructions'] = int(re.search(r'Collected\s*:\s*(\d+)', text)[1])
            else:
                row['allocated_bytes'] = int(re.search(r'Total:\s*([\d,]+) bytes', text)[1].replace(',', ''))
                row['peak_live_bytes'] = int(re.search(r'At t-gmax:\s*([\d,]+) bytes', text)[1].replace(',', ''))
            results['measurements'].append(row)
            print(json.dumps(row), flush=True)
        assert (out / f'{workload}-{tool}-0.diagnostics').read_bytes() == (out / f'{workload}-{tool}-1.diagnostics').read_bytes(), f'diagnostics changed: {workload}'
    (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
