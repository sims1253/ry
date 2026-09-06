#!/usr/bin/env python3
"""Measure the fixed corpus sample; compare only compatible instruction counts."""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import signal
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "ecosystem/instruction-packages.txt"
COMMAND = ["check", "--exit-zero", "--output-format", "json", "<package>/R"]
ENVIRONMENT = {"RY_NO_INSTALLED_LIBRARIES": "1", "RAYON_NUM_THREADS": "1", "LC_ALL": "C", "TZ": "UTC"}


def run(command, cwd=None, timeout=120, env=None):
    return subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True,
                          check=True, timeout=timeout).stdout.strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def samples():
    result = []
    for line in MANIFEST.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, url, revision = line.split()
        if not re.fullmatch(r"[A-Za-z][A-Za-z0-9._-]*", name) or not re.fullmatch(r"[0-9a-f]{40}", revision):
            raise ValueError("Sample names and full commit pins are required")
        if any(row["package"] == name for row in result):
            raise ValueError(f"Duplicate sample: {name}")
        result.append({"package": name, "url": url, "revision": revision})
    if not result:
        raise ValueError("Instruction sample is empty")
    return result


def parse_perf(text):
    for line in text.splitlines():
        fields = line.split(";")
        if len(fields) >= 3 and fields[2].strip() == "instructions:u":
            value = fields[0].strip()
            # perf prints <not supported>/<not counted> instead of a count.
            if not value.isdecimal() or int(value) <= 0:
                raise ValueError("perf did not return a positive instruction count")
            # Reject multiplexed/scaled counters rather than call them exact counts.
            if len(fields) < 5 or fields[4].strip().rstrip("%") not in ("100", "100.00"):
                raise ValueError("perf instruction counter was multiplexed")
            return int(value)
    raise ValueError("perf instruction count is missing")


def parse_callgrind(text):
    events = re.search(r"^events:\s*(.+)$", text, re.MULTILINE)
    totals = re.findall(r"^totals:\s*(.+)$", text, re.MULTILINE)
    if not events or not totals or "Ir" not in events[1].split():
        raise ValueError("Callgrind Ir total is missing")
    columns = totals[-1].split()
    index = events[1].split().index("Ir")
    if index >= len(columns):
        raise ValueError("Callgrind Ir total is incomplete")
    value = int(columns[index])
    if value <= 0:
        raise ValueError("Callgrind returned a nonpositive instruction count")
    return value


def counter_command(backend, output, command):
    if backend == "perf":
        return ["perf", "stat", "--no-big-num", "--no-scale", "-x", ";", "-e", "instructions:u", "-o", str(output), "--", *command]
    return ["valgrind", "--command-line-only=yes", "--tool=callgrind", "--quiet", "--cache-sim=no", "--branch-sim=no",
            "--collect-jumps=no", "--callgrind-out-file=" + str(output), *command]


def count(backend, command, cwd, timeout, env):
    with tempfile.TemporaryDirectory(prefix="ry-counter-") as directory:
        output = Path(directory) / "counter.txt"
        counter_env = dict(ENVIRONMENT, PATH=env.get("PATH", os.defpath), HOME=directory)
        measured = subprocess.Popen(counter_command(backend, output, command), cwd=cwd, env=counter_env,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                                    text=True, start_new_session=True)
        try:
            _, stderr = measured.communicate(timeout=timeout)
        except subprocess.TimeoutExpired:
            os.killpg(measured.pid, signal.SIGKILL)
            measured.communicate()
            raise
        if measured.returncode:
            raise RuntimeError(f"counter/checker exited {measured.returncode}: {stderr[-2000:].strip()}")
        return (parse_perf if backend == "perf" else parse_callgrind)(output.read_text())


def select_backend(requested, env):
    failures = []
    for backend in (["perf", "callgrind"] if requested == "auto" else [requested]):
        executable = "perf" if backend == "perf" else "valgrind"
        if not shutil.which(executable):
            failures.append(f"{executable} is not installed")
            continue
        try:
            count(backend, [shutil.which("true")], ROOT, 30, env)
            version = run([executable, "--version"], env=env)
            return backend, version, failures
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
            failures.append(f"{backend}: {error}")
    raise RuntimeError("No usable instruction counter: " + "; ".join(failures))


def prepare_sample(sample, cache):
    directory = cache / (sample["package"] + "-" + sample["revision"])
    if not (directory / ".git").exists():
        directory.mkdir(parents=True, exist_ok=True)
        run(["git", "init", "--quiet", str(directory)])
        run(["git", "-C", str(directory), "fetch", "--quiet", "--depth", "1", sample["url"], sample["revision"]])
        run(["git", "-C", str(directory), "checkout", "--quiet", "--detach", "FETCH_HEAD"])
    if run(["git", "rev-parse", "HEAD"], cwd=directory) != sample["revision"]:
        raise ValueError(f"Sample checkout changed: {directory}")
    config = directory / "R/ry.toml"
    # This file belongs to the harness, not to the pinned package.
    if run(["git", "ls-files", "R/ry.toml"], cwd=directory):
        raise ValueError("Pinned sample contains R/ry.toml; review the measurement contract")
    config.unlink(missing_ok=True)
    if run(["git", "status", "--porcelain", "--untracked-files=all"], cwd=directory):
        raise ValueError(f"Sample checkout is dirty: {directory}")
    config.write_text("")  # Stop config discovery before it reaches the host project.
    return directory, run(["git", "rev-parse", "HEAD^{tree}"], cwd=directory)


def measure(args, selected_samples):
    source = args.source.resolve()
    env = dict(os.environ, **ENVIRONMENT, GIT_TERMINAL_PROMPT="0")
    if env.get("CARGO_BUILD_TARGET"):
        raise ValueError("Unset CARGO_BUILD_TARGET for the native instruction benchmark")
    backend, backend_version, fallback = select_backend(args.backend, env)
    # Cargo must build the measured source, never an unrelated executable on PATH.
    build = ["cargo", "build", "--release", "--locked", "-p", "ry-cli", "--bin", "ry",
             "--target-dir", str(args.target_dir.resolve())]
    subprocess.run(build, cwd=source, env=env, check=True, timeout=600)
    binary = args.target_dir.resolve() / "release/ry"
    rustc = run(["rustc", "-vV"], cwd=source, env=env)
    cpu = "unknown"
    if Path("/proc/cpuinfo").exists():
        cpu = next((line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
                    if line.startswith("model name")), "unknown")
    metadata = {
        "backend": backend, "backend_version": backend_version,
        "architecture": platform.machine(), "system": platform.system(), "cpu": cpu,
        "libc": list(platform.libc_ver()), "rustc": rustc,
        "cargo": run(["cargo", "--version"], cwd=source, env=env),
        "cc": run(["cc", "--version"], env=env).splitlines()[0],
        "build_profile": "release", "build_flags": {key: env[key] for key in sorted(env) if
            key in ("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CFLAGS", "CXXFLAGS", "LDFLAGS", "CC", "CXX")
            or key.startswith(("CARGO_PROFILE_", "CARGO_BUILD_", "CARGO_TARGET_"))},
        "command": COMMAND, "environment": ENVIRONMENT, "config": "empty R/ry.toml",
        "counter_environment": "fixed variables plus tool PATH and empty temporary HOME",
        "manifest_sha256": digest(MANIFEST), "harness_sha256": digest(Path(__file__)), "repetitions": args.repetitions,
    }
    ledger = {"schema_version": 1, "created_at": datetime.now(timezone.utc).isoformat(),
              "source_revision": run(["git", "rev-parse", "HEAD"], cwd=source),
              "source_dirty": bool(run(["git", "status", "--porcelain", "--untracked-files=all", "--",
                                        "crates", "Cargo.toml", "Cargo.lock", "build.rs", ".cargo",
                                        "rust-toolchain", "rust-toolchain.toml"], cwd=source)),
              "binary_sha256": digest(binary), "cargo_lock_sha256": digest(source / "Cargo.lock"),
              "metadata": metadata, "fallback_notes": fallback, "packages": []}
    deadline = time.monotonic() + args.budget
    cache = args.cache.resolve()
    cache.mkdir(parents=True, exist_ok=True)
    for sample in selected_samples:
        row = dict(sample, status="unavailable")
        try:
            if time.monotonic() >= deadline:
                raise TimeoutError("measurement budget exhausted")
            directory, tree = prepare_sample(sample, cache)
            row["tree"] = tree
            values = []
            for _ in range(args.repetitions):
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("measurement budget exhausted")
                values.append(count(backend, [str(binary), *COMMAND[:-1], str(directory / "R")], directory,
                                    min(args.package_timeout, remaining), env))
            row.update(status="measured", counts=values, instructions=statistics.median(values))
        except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
            row["reason"] = str(error)
        ledger["packages"].append(row)
        print(f"{row['package']}: {row.get('instructions', row.get('reason'))}", file=sys.stderr)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(ledger, indent=2) + "\n")
    return 0 if all(row["status"] == "measured" for row in ledger["packages"]) else 1


def compare(baseline, current, threshold):
    if not math.isfinite(threshold) or threshold < 0:
        raise ValueError("Threshold must be a finite, nonnegative percentage")
    lines = ["## Corpus instruction counts", "", f"Warning threshold: +{threshold:g}%. Counts include checker startup and JSON output.", ""]
    if any(ledger["metadata"].get("backend") == "unavailable" for ledger in (baseline, current)):
        return "\n".join(lines + ["**UNAVAILABLE:** a counter or build failed. See the ledger reasons and measurement logs.", ""])
    mismatch = [key for key in sorted(set(baseline["metadata"]) | set(current["metadata"]))
                if baseline["metadata"].get(key) != current["metadata"].get(key)]
    if baseline.get("schema_version") != 1 or current.get("schema_version") != 1:
        mismatch.append("schema_version")
    if mismatch:
        return "\n".join(lines + ["**INCOMPARABLE:** " + ", ".join(mismatch), "No regression claim is made.", ""])
    lines += [f"Baseline: `{baseline['source_revision']}`. Current: `{current['source_revision']}`.", "",
              "| Package | Baseline | Current | Delta | Status |", "| --- | ---: | ---: | ---: | --- |"]
    previous = {row["package"]: row for row in baseline["packages"]}
    current_names = {row["package"] for row in current["packages"]}
    for name in sorted(previous.keys() - current_names):
        lines.append(f"| {name} | - | - | - | UNAVAILABLE current row |")
    for row in current["packages"]:
        old = previous.get(row["package"])
        if row["status"] != "measured" or (old and old["status"] != "measured"):
            lines.append(f"| {row['package']} | - | - | - | UNAVAILABLE |")
        elif not old or any(old.get(key) != row.get(key) for key in ("revision", "tree", "url")):
            lines.append(f"| {row['package']} | - | - | - | INCOMPARABLE sample |")
        else:
            if any(type(item.get("instructions")) not in (int, float) or not math.isfinite(item["instructions"])
                   or item["instructions"] <= 0 for item in (old, row)):
                raise ValueError(f"Invalid measured count for {row['package']}")
            delta = 100 * (row["instructions"] / old["instructions"] - 1)
            status = "WARNING" if delta > threshold else "ok"
            lines.append(f"| {row['package']} | {old['instructions']:,.0f} | {row['instructions']:,.0f} | {delta:+.2f}% | {status} |")
    return "\n".join(lines) + "\n"


def positive(value):
    value = int(value)
    if value <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    measure_parser = commands.add_parser("measure")
    measure_parser.add_argument("--source", type=Path, default=ROOT)
    measure_parser.add_argument("--output", type=Path, required=True)
    measure_parser.add_argument("--backend", choices=["auto", "perf", "callgrind"], default="auto")
    measure_parser.add_argument("--target-dir", type=Path, default=ROOT / "target/instructions")
    measure_parser.add_argument("--cache", type=Path, default=ROOT / "ecosystem/.cache/instructions")
    measure_parser.add_argument("--budget", type=positive, default=180, help="total measurement budget in seconds (build has a separate 600-second cap)")
    measure_parser.add_argument("--package-timeout", type=positive, default=60)
    measure_parser.add_argument("--repetitions", type=positive, default=3)
    compare_parser = commands.add_parser("compare")
    compare_parser.add_argument("baseline", type=Path)
    compare_parser.add_argument("current", type=Path)
    compare_parser.add_argument("--threshold", type=float, default=10.0)
    args = parser.parse_args()
    selected_samples = []
    try:
        if args.command == "measure":
            selected_samples = samples()
            return measure(args, selected_samples)
        print(compare(json.loads(args.baseline.read_text()), json.loads(args.current.read_text()), args.threshold), end="")
        return 0  # Growth is warn-only; measurement/format errors still fail.
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError) as error:
        print(f"instructions: {error}", file=sys.stderr)
        if args.command == "measure":
            # Even an unavailable counter or failed build gets an explicit report.
            ledger = {"schema_version": 1, "created_at": datetime.now(timezone.utc).isoformat(),
                      "metadata": {"backend": "unavailable"}, "reason": str(error),
                      "packages": [dict(sample, status="unavailable", reason=str(error)) for sample in selected_samples]}
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(ledger, indent=2) + "\n")
        return 1


if __name__ == "__main__":
    sys.exit(main())
