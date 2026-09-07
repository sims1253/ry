#!/usr/bin/env python3
"""Profile the isolated experiment in a new output directory."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--out", type=Path, required=True)
parser.add_argument("--target-dir", type=Path)
parser.add_argument("--workloads", nargs="+", choices=["sparse", "dense", "alternating", "corpus"], default=["dense", "corpus", "alternating", "sparse"])
parser.add_argument("--tools", nargs="+", choices=["callgrind", "dhat"], default=["callgrind"])
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
args.out.mkdir(parents=True, exist_ok=False)
out = args.out.resolve()
for kind in ["sparse", "dense", "alternating"]:
    directory = out / kind
    directory.mkdir()
    source = "f <- function(flag) {\n" + "".join(f"x{i} <- {i}L\n" for i in range(1024))
    for depth in range(24):
        source += "if (flag) {\n"
        value = '"changed"' if kind != "alternating" or depth % 2 == 0 else "1L"
        names = [depth] if kind == "sparse" else range(1024)
        source += "".join(f"x{i} <- {value}\n" for i in names)
    (directory / "branches.R").write_text(source + "}\n" * 24 + "x0\n}\nf(TRUE)\n")
corpus = out / "corpus"
corpus.mkdir()
for source in (repo / "crates/ry-checker/testdata").glob("*.R"):
    shutil.copyfile(source, corpus / source.name)
harness = out / "harness"
(harness / "src").mkdir(parents=True)
shutil.copyfile(Path(__file__).with_name("main.rs"), harness / "src/main.rs")
(harness / "Cargo.toml").write_text(
    '[package]\nname="ry-persistent-probe"\nversion="0.1.0"\nedition="2024"\n'
    '[workspace]\n[features]\npersistent-scope=["ry-checker/persistent-scope"]\n[dependencies]\n'
    + "\n".join(f'{name}={{path={json.dumps(str(repo / "crates" / name))}}}' for name in ["ry-checker", "ry-core"])
    + '\n[profile.release]\ndebug=0\n'
)
shutil.copyfile(Path(__file__).with_name("Cargo.lock"), harness / "Cargo.lock")
target_dir = (args.target_dir or out / "target").resolve()
env = dict(os.environ, CARGO_TARGET_DIR=str(target_dir), RAYON_NUM_THREADS="1", RY_NO_INSTALLED_LIBRARIES="1")
binaries = {}
for mode in ["std", "im"]:
    command = ["cargo", "build", "--release", "--locked", "--manifest-path", str(harness / "Cargo.toml")]
    if mode == "im":
        command += ["--features", "persistent-scope"]
    subprocess.run(command, env=env, check=True)
    binaries[mode] = out / f"{mode}-probe"
    shutil.copyfile(target_dir / "release/ry-persistent-probe", binaries[mode])
    binaries[mode].chmod(0o755)
results = {
    "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
    "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
    "valgrind": subprocess.check_output(["valgrind", "--version"], text=True).strip(),
    "settings": {"rayon_threads": 1, "no_installed_libraries": True},
    "inputs": {}, "measurements": [],
}
for workload in args.workloads:
    directory = out / workload
    digest = hashlib.sha256()
    for source in sorted(directory.glob("*.R")):
        digest.update(source.name.encode() + b"\0" + source.read_bytes())
    results["inputs"][workload] = {"sha256": digest.hexdigest(), "files": len(list(directory.glob("*.R")))}
    for tool in args.tools:
        for enabled in ["std", "im"]:
            stem = out / f"{workload}-{tool}-{enabled}"
            environment = env
            flag = f"--callgrind-out-file={stem}.profile" if tool == "callgrind" else f"--dhat-out-file={stem}.profile"
            with stem.with_suffix(".log").open("w") as log:
                subprocess.run(["valgrind", "--command-line-only=yes", f"--tool={tool}", flag, str(binaries[enabled]), str(directory), str(stem.with_suffix(".diagnostics"))], env=environment, stdout=log, stderr=log, check=True)
            text = stem.with_suffix(".log").read_text()
            row = {"workload": workload, "tool": tool, "mode": enabled}
            if tool == "callgrind":
                row["instructions"] = int(re.search(r"Collected\s*:\s*(\d+)", text)[1])
            else:
                row["allocated_bytes"] = int(re.search(r"Total:\s*([\d,]+) bytes", text)[1].replace(",", ""))
                row["peak_live_bytes"] = int(re.search(r"At t-gmax:\s*([\d,]+) bytes", text)[1].replace(",", ""))
            results["measurements"].append(row)
            print(json.dumps(row), flush=True)
        baseline = out / f"{workload}-{tool}-std.diagnostics"
        changed = out / f"{workload}-{tool}-im.diagnostics"
        assert baseline.read_bytes() == changed.read_bytes(), f"diagnostics changed: {workload}"
    (out / "results.json").write_text(json.dumps(results, indent=2) + "\n")
