#!/usr/bin/env python3
"""Run a local, pinned checker experiment and retain its evidence."""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
STATUSES = ("passed", "failed", "skipped", "unavailable", "incomparable", "cancelled")
GATES = [
    ("workspace", ["cargo", "test", "--workspace"], False),
    ("clippy", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"], False),
    ("format", ["cargo", "fmt", "--all", "--", "--check"], False),
    ("oracle", ["cargo", "test", "-p", "ry-checker", "--test", "oracle", "--", "--include-ignored"], True),
]
IDENTITY_FIELDS = ("code", "line", "column")


class Cancelled(Exception):
    pass


def cancel(_signal, _frame):
    raise Cancelled("interrupted")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(*args, cwd=ROOT):
    return subprocess.run(["git", *args], cwd=cwd, text=True, capture_output=True, check=True).stdout.strip()


def resolve_revision(ref):
    if not isinstance(ref, str) or not ref or ref.startswith("-"):
        raise ValueError("reference and candidate must be Git revisions")
    revision = git("rev-parse", "--verify", f"{ref}^{{commit}}")
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError(f"Git did not resolve {ref} to a commit")
    return revision


def relative_file(value):
    if not isinstance(value, str) or not value or Path(value).is_absolute() or ".." in Path(value).parts:
        raise ValueError(f"expected a repository-relative file: {value!r}")
    return value


def identity(row):
    return tuple(row[key] for key in IDENTITY_FIELDS)


def identity_dict(key):
    return dict(zip(IDENTITY_FIELDS, key))


def validate_manifest(manifest):
    if manifest.get("schema_version") != 1:
        raise ValueError("manifest schema_version must be 1")
    for key in ("hypothesis", "expected_change", "reference", "candidate"):
        if not isinstance(manifest.get(key), str) or not manifest[key].strip():
            raise ValueError(f"manifest needs {key}")
    if not isinstance(manifest.get("invariants"), list) or not manifest["invariants"] or not all(
        isinstance(item, str) and item.strip() for item in manifest["invariants"]):
        raise ValueError("manifest needs correctness invariants")
    if not isinstance(manifest.get("r_packages", []), list):
        raise ValueError("r_packages must be a list")
    fixtures = manifest.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        raise ValueError("manifest needs selected fixtures")
    roles = set()
    names = set()
    for fixture in fixtures:
        if fixture.get("role") not in ("witness", "quiet_control"):
            raise ValueError("fixture role must be witness or quiet_control")
        roles.add(fixture["role"])
        name = fixture.get("name")
        if not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name) or name in names:
            raise ValueError("fixture names must be unique simple identifiers")
        names.add(name)
        if not relative_file(fixture.get("path")).endswith(".R"):
            raise ValueError("first increment accepts R files as fixtures")
        workload = fixture.get("workload")
        if workload is not None:
            if not isinstance(workload, dict) or not isinstance(workload.get("repository"), str) or not Path(workload["repository"]).is_absolute() or not re.fullmatch(r"[0-9a-f]{40}", workload.get("revision", "")):
                raise ValueError("workload needs a local absolute repository path and full commit pin")
        for change in ("expected_additions", "expected_removals"):
            if not isinstance(fixture.get(change, []), list):
                raise ValueError(f"{name}.{change} must be a list")
            for row in fixture.get(change, []):
                identity(row)
        if "triage" in fixture:
            triage = fixture["triage"]
            relative_file(triage["ledger"])
            if not triage.get("package") or not triage.get("path"):
                raise ValueError("triage needs ledger, package and package-relative path")
            if workload is None or triage["path"] != fixture["path"]:
                raise ValueError("triage labels require the pinned workload file at the ledger path")
    if roles != {"witness", "quiet_control"}:
        raise ValueError("select a witness and an adjacent quiet control")
    for check in manifest.get("targeted_checks", []):
        if not isinstance(check, dict):
            raise ValueError("targeted checks must be objects")
        if not isinstance(check.get("name"), str) or not re.fullmatch(r"[A-Za-z0-9_-]+", check["name"]) or check.get("side", "candidate") not in ("reference", "candidate", "both"):
            raise ValueError("targeted check needs name and valid side")
        argv = check.get("argv")
        if not isinstance(argv, list) or not argv or not all(isinstance(x, str) and x for x in argv):
            raise ValueError("targeted check argv must be a nonempty string array")
    instructions = manifest.get("instructions", {})
    if not isinstance(instructions, dict) or instructions.get("backend", "auto") not in ("auto", "perf", "callgrind") or type(instructions.get("enabled", False)) is not bool:
        raise ValueError("invalid instruction measurement configuration")
    for key in ("budget", "repetitions"):
        if key in instructions and (type(instructions[key]) is not int or instructions[key] <= 0):
            raise ValueError(f"instructions.{key} must be a positive integer")


def labels_for(fixture, source):
    triage = fixture.get("triage")
    if not triage:
        return {}, None
    path = source / triage["ledger"]
    ledger = json.loads(path.read_text())
    package = next((row for row in ledger["packages"] if row["name"] == triage["package"]), None)
    if package is None or package["commit"] != fixture["workload"]["revision"]:
        raise ValueError("triage ledger package pin differs from the selected workload")
    labels = {}
    for row in ledger["findings"]:
        if row["package"] == triage["package"] and row["path"] == triage["path"]:
            key = identity(row)
            if key in labels and labels[key] != row["label"]:
                raise ValueError(f"conflicting triage labels: {key}")
            labels[key] = row["label"]
    return labels, sha256(path)


def diff_findings(reference, candidate, labels):
    """Use the same code/path/line/column identity as the corpus ledger.

    A selected fixture is one file, so its path is represented by fixture name
    in the caller. Counter subtraction preserves duplicate findings.
    """
    old = Counter(identity(row) for row in reference)
    new = Counter(identity(row) for row in candidate)
    def rows(delta):
        return [dict(identity_dict(key), label=labels.get(key, "unreviewed"))
                for key in sorted(delta.elements())]
    return rows(new - old), rows(old - new)


def evaluate_fixture(fixture, reference, candidate, labels):
    added, removed = diff_findings(reference, candidate, labels)
    expected_added = Counter(identity(row) for row in fixture.get("expected_additions", []))
    expected_removed = Counter(identity(row) for row in fixture.get("expected_removals", []))
    actual_added = Counter(identity(row) for row in added)
    actual_removed = Counter(identity(row) for row in removed)
    lost_tp = any(row["label"] == "true_positive" for row in removed)
    passed = actual_added == expected_added and actual_removed == expected_removed and not lost_tp
    reason = ("reviewed true positive removed" if lost_tp else
              "finding delta differs from manifest expectation" if not passed else "expected finding delta")
    return added, removed, "passed" if passed else "failed", reason


def compare_instructions(old, new):
    if old.get("metadata", {}).get("backend") == "unavailable" or new.get("metadata", {}).get("backend") == "unavailable":
        return "unavailable", "counter or build unavailable"
    if old.get("schema_version") != 1 or new.get("schema_version") != 1:
        return "incomparable", "ledger schema differs"
    if old.get("metadata") != new.get("metadata"):
        return "incomparable", "measurement metadata differs"
    earlier = {row["package"]: row for row in old.get("packages", [])}
    later = {row["package"]: row for row in new.get("packages", [])}
    if not earlier or not later:
        return "unavailable", "measurement rows missing"
    if earlier.keys() != later.keys():
        return "incomparable", "workload package set differs"
    for name in earlier:
        a, b = earlier[name], later[name]
        if any(a.get(key) != b.get(key) for key in ("revision", "tree", "url")):
            return "incomparable", f"{name} workload pin differs"
        if a.get("status") != "measured" or b.get("status") != "measured":
            return "unavailable", f"{name} measurement unavailable"
    return "passed", "comparable counts; inspect raw samples and uncertainty"


class Experiment:
    def __init__(self, manifest_path, output, profile):
        self.manifest_path = manifest_path.resolve()
        self.output = output.resolve()
        self.profile = profile
        self.manifest = json.loads(self.manifest_path.read_text())
        validate_manifest(self.manifest)
        self.report = {"schema_version": 1, "profile": profile, "fully_validated": False,
                       "status": "skipped", "created_at": datetime.now(timezone.utc).isoformat(),
                       "manifest_sha256": sha256(self.manifest_path), "manifest": self.manifest,
                       "revisions": {}, "metadata": {}, "stages": [], "fixtures": []}
        self.output.mkdir(parents=True, exist_ok=False)
        (self.output / "raw").mkdir()
        inherited = ("PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME", "RUSTUP_TOOLCHAIN",
                     "R_LIBS_USER", "R_LIBS_SITE")
        self.env = {key: os.environ[key] for key in inherited if key in os.environ}
        self.env.update(LC_ALL="C", TZ="UTC", RAYON_NUM_THREADS="1",
                        RY_NO_INSTALLED_LIBRARIES="1", CARGO_PROFILE_DEV_DEBUG="0",
                        CARGO_PROFILE_TEST_DEBUG="0", CARGO_BUILD_JOBS="4",
                        GIT_TERMINAL_PROMPT="0")

    def stage(self, name, status, **details):
        if status not in STATUSES:
            raise ValueError(status)
        row = dict(name=name, status=status, **details)
        self.report["stages"].append(row)
        return row

    def command(self, name, argv, cwd, *, side="candidate", requires_r=False, timeout=1800,
                cargo_target=True):
        path = self.output / "raw" / f"{len(self.report['stages']):03d}-{name}"
        env = dict(self.env)
        if cargo_target:
            env["CARGO_TARGET_DIR"] = str(self.output / f"target-{side}")
        record = {"argv": argv, "cwd": str(cwd), "environment": {key: env[key] for key in sorted(env)},
                  "stdout": str(path.with_suffix(".stdout")),
                  "stderr": str(path.with_suffix(".stderr")), "source_revision": self.report["revisions"].get(side)}
        if requires_r and not shutil.which("Rscript", path=env.get("PATH")):
            return self.stage(name, "unavailable", reason="Rscript is not installed", **record)
        try:
            proc = subprocess.Popen(argv, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                    start_new_session=True)
            try:
                out, err = proc.communicate(timeout=timeout)
            except (Cancelled, KeyboardInterrupt, subprocess.TimeoutExpired) as error:
                os.killpg(proc.pid, signal.SIGKILL)
                out, err = proc.communicate()
                path.with_suffix(".stdout").write_bytes(out)
                path.with_suffix(".stderr").write_bytes(err)
                if isinstance(error, subprocess.TimeoutExpired):
                    return self.stage(name, "failed", reason=f"timed out after {timeout}s", **record)
                self.stage(name, "cancelled", reason="interrupted", **record)
                raise Cancelled("interrupted") from error
            path.with_suffix(".stdout").write_bytes(out)
            path.with_suffix(".stderr").write_bytes(err)
            return self.stage(name, "passed" if proc.returncode == 0 else "failed",
                              exit_code=proc.returncode, **record)
        except FileNotFoundError as error:
            return self.stage(name, "unavailable", reason=str(error), **record)

    def setup(self):
        for side in ("reference", "candidate"):
            revision = resolve_revision(self.manifest[side])
            self.report["revisions"][side] = revision
            source = self.output / side
            git("worktree", "add", "--detach", str(source), revision)
            self.report["metadata"][side] = {
                "tree": git("rev-parse", "HEAD^{tree}", cwd=source),
                "cargo_lock_sha256": sha256(source / "Cargo.lock"),
                "source": str(source),
            }
        self.report["metadata"]["workloads"] = {}
        for fixture in self.manifest["fixtures"]:
            workload = fixture.get("workload")
            if workload is None:
                continue
            repository = Path(workload["repository"]).resolve()
            revision = git("rev-parse", "--verify", f"{workload['revision']}^{{commit}}", cwd=repository)
            if revision != workload["revision"]:
                raise ValueError(f"workload pin did not resolve exactly: {fixture['name']}")
            path = self.output / "workloads" / fixture["name"]
            path.parent.mkdir(exist_ok=True)
            git("worktree", "add", "--detach", str(path), revision, cwd=repository)
            self.report["metadata"]["workloads"][fixture["name"]] = {
                "revision": revision, "tree": git("rev-parse", "HEAD^{tree}", cwd=path),
                "source_repository": str(repository), "checkout": str(path),
            }
        self.report["metadata"]["environment"] = {key: self.env[key] for key in sorted(self.env)}
        self.report["metadata"]["toolchain"] = {}
        for name, argv in (("rustc", ["rustc", "-vV"]), ("cargo", ["cargo", "--version"]),
                           ("R", ["Rscript", "--version"])):
            if shutil.which(argv[0]):
                result = subprocess.run(argv, capture_output=True, text=True)
                self.report["metadata"]["toolchain"][name] = (result.stdout or result.stderr).strip()
            else:
                self.report["metadata"]["toolchain"][name] = None
        packages = self.manifest.get("r_packages", [])
        if packages and shutil.which("Rscript"):
            if not all(re.fullmatch(r"[A-Za-z][A-Za-z0-9.]*", name) for name in packages):
                raise ValueError("r_packages must contain package names")
            expression = "for (p in c(" + ",".join(json.dumps(p) for p in packages) + ")) cat(p, if (requireNamespace(p, quietly=TRUE)) as.character(packageVersion(p)) else 'unavailable', '\\n')"
            result = subprocess.run(["Rscript", "-e", expression], capture_output=True, text=True, timeout=30)
            self.report["metadata"]["r_packages"] = result.stdout.splitlines() if result.returncode == 0 else ["unavailable"]

    def run_checks(self):
        if self.profile == "full":
            keep_going = True
            for name, argv, needs_r in GATES:
                if keep_going:
                    row = self.command(name, argv, self.output / "candidate", requires_r=needs_r)
                    keep_going = row["status"] == "passed"
                else:
                    self.stage(name, "skipped", reason="earlier required gate did not pass")
        for check in self.manifest.get("targeted_checks", []):
            for side in (("reference", "candidate") if check.get("side") == "both" else (check.get("side", "candidate"),)):
                self.command(f"targeted-{check['name']}-{side}", check["argv"], self.output / side,
                             side=side, requires_r=check.get("requires_r", False))

    def run_fixtures(self):
        builds = {}
        for side in ("reference", "candidate"):
            source = self.output / side
            row = self.command(f"build-{side}", ["cargo", "build", "--locked", "-p", "ry-cli", "--bin", "ry"], source, side=side)
            if row["status"] != "passed":
                self.stage(f"fixtures-{side}", "skipped", reason="checker build failed")
                return
            binary = self.output / f"target-{side}" / "debug" / "ry"
            builds[side] = binary
            self.report["metadata"][side]["binary_sha256"] = sha256(binary)
        for fixture in self.manifest["fixtures"]:
            name = fixture["name"]
            labels, ledger_hash = labels_for(fixture, self.output / "candidate")
            item = {"name": name, "role": fixture["role"], "path": fixture["path"],
                    "triage_ledger_sha256": ledger_hash, "reference": [], "candidate": []}
            self.report["fixtures"].append(item)
            for side in ("reference", "candidate"):
                source = self.output / side
                fixture_root = self.output / "workloads" / name if "workload" in fixture else source
                file = fixture_root / fixture["path"]
                if not file.is_file() or not file.resolve().is_relative_to(fixture_root.resolve()):
                    self.stage(f"fixture-{name}-{side}", "unavailable", reason="fixture missing from source")
                    return
                item[f"{side}_sha256"] = sha256(file)
                argument = str(file) if "workload" in fixture else fixture["path"]
                row = self.command(f"fixture-{name}-{side}", [str(builds[side]), "check", "--exit-zero", "--output-format", "json", argument],
                                   source, side=side)
                if row["status"] != "passed":
                    return
                try:
                    findings = json.loads(Path(row["stdout"]).read_text())
                    if not isinstance(findings, list) or not all(all(key in finding for key in IDENTITY_FIELDS) for finding in findings):
                        raise ValueError("invalid diagnostic JSON")
                    item[side] = findings
                except (ValueError, TypeError) as error:
                    self.stage(f"fixture-json-{name}-{side}", "failed", reason=str(error))
                    return
            added, removed, status, reason = evaluate_fixture(fixture, item["reference"], item["candidate"], labels)
            item["added"], item["removed"] = added, removed
            self.stage(f"fixture-diff-{name}", status, reason=reason)

    def run_instructions(self):
        settings = self.manifest.get("instructions", {})
        if not settings.get("enabled", False):
            self.stage("instructions", "skipped", reason="not selected in manifest")
            return
        harness = self.output / "candidate" / "ecosystem" / "instructions.py"
        packages = self.output / "candidate" / "ecosystem" / "instruction-packages.txt"
        self.report["metadata"]["instruction_harness_sha256"] = sha256(harness)
        self.report["metadata"]["instruction_packages_sha256"] = sha256(packages)
        reference_packages = self.output / "reference" / "ecosystem" / "instruction-packages.txt"
        if sha256(reference_packages) != sha256(packages):
            self.stage("instructions", "incomparable", reason="reference and candidate workload pins differ")
            return
        ledgers = {}
        for side in ("reference", "candidate"):
            ledger = self.output / f"instructions-{side}.json"
            command = [sys.executable, str(harness), "measure", "--source", str(self.output / side),
                       "--target-dir", str(self.output / f"instructions-target-{side}"),
                       "--cache", str(self.output / "instruction-cache"), "--output", str(ledger),
                       "--backend", settings.get("backend", "auto"),
                       "--budget", str(settings.get("budget", 180)),
                       "--repetitions", str(settings.get("repetitions", 3))]
            row = self.command(f"instructions-measure-{side}", command, self.output / side, side=side,
                               timeout=900, cargo_target=False)
            if ledger.exists():
                ledgers[side] = json.loads(ledger.read_text())
            if row["status"] != "passed":
                reason = ledgers.get(side, {}).get("reason", "measurement command failed")
                unavailable_rows = [item for item in ledgers.get(side, {}).get("packages", [])
                                    if item.get("status") == "unavailable"]
                state = "unavailable" if "No usable instruction counter" in reason or (
                    unavailable_rows and not ledgers.get(side, {}).get("reason")) else row["status"]
                if unavailable_rows and reason == "measurement command failed":
                    reason = "; ".join(f"{item['package']}: {item.get('reason', 'unavailable')}" for item in unavailable_rows)
                row["status"] = state
                row["reason"] = reason
                self.stage("instructions", state, reason=reason)
                return
        state, reason = compare_instructions(ledgers["reference"], ledgers["candidate"])
        row = self.command("instructions-compare", [sys.executable, str(harness), "compare",
                           str(self.output / "instructions-reference.json"), str(self.output / "instructions-candidate.json")],
                           self.output / "candidate", cargo_target=False)
        if row["status"] != "passed":
            state, reason = "failed", "instruction compare command failed"
        self.stage("instructions", state, reason=reason, raw_report=row.get("stdout"))

    def finish(self):
        selected = [row for row in self.report["stages"] if row["name"] != "instructions"]
        states = [row["status"] for row in self.report["stages"]]
        if "cancelled" in states:
            status = "cancelled"
        elif "failed" in states:
            status = "failed"
        elif "unavailable" in states:
            status = "unavailable"
        elif "incomparable" in states:
            status = "incomparable"
        elif any(row["status"] == "skipped" and row["name"] != "instructions" for row in selected):
            status = "skipped"
        else:
            status = "passed"
        self.report["status"] = status
        self.report["fully_validated"] = (self.profile == "full" and status == "passed" and
            all(any(row["name"] == name and row["status"] == "passed" for row in selected) for name, _, _ in GATES) and
            any(row["name"] == "instructions" and row["status"] == "passed" for row in self.report["stages"]))
        (self.output / "report.json").write_text(json.dumps(self.report, indent=2) + "\n")
        lines = ["# Checker experiment", "", f"Hypothesis: {self.manifest['hypothesis']}",
                 f"Expected change: {self.manifest['expected_change']}", "",
                 f"Profile: **{self.profile}**; status: **{status}**; fully validated: **{self.report['fully_validated']}**.", "",
                 "| Source | Resolved commit |", "| --- | --- |"]
        lines += [f"| {side} | `{revision}` |" for side, revision in self.report["revisions"].items()]
        lines += ["", "## Stages", "", "| Stage | Status | Reason |", "| --- | --- | --- |"]
        for stage in self.report["stages"]:
            lines.append(f"| {stage['name']} | {stage['status']} | {stage.get('reason', '')} |")
        lines += ["", "## Selected findings", ""]
        for fixture in self.report["fixtures"]:
            lines.append(f"### {fixture['name']} ({fixture['role']})")
            lines.append(f"Reference: {len(fixture['reference'])}; candidate: {len(fixture['candidate'])}.")
            for direction in ("removed", "added"):
                for finding in fixture.get(direction, []):
                    lines.append(f"- {direction}: `{finding['code']}:{finding['line']}:{finding['column']}` — {finding['label']}")
            lines.append("")
        lines += ["Instruction counts retain every sample in the raw ledgers. Comparability requires matching backend, toolchain metadata, and workload pins; timings and counts have measurement uncertainty.",
                  "", "Raw commands, stdout, stderr, source and binary hashes are in `report.json` and `raw/`.", ""]
        (self.output / "report.md").write_text("\n".join(lines))
        return 0 if status == "passed" else 1

    def run(self):
        try:
            self.setup()
            self.run_checks()
            self.run_fixtures()
            self.run_instructions()
        except (Cancelled, KeyboardInterrupt):
            self.stage("run", "cancelled", reason="interrupted")
        except (OSError, ValueError, subprocess.CalledProcessError, json.JSONDecodeError) as error:
            self.stage("run", "failed", reason=str(error))
        return self.finish()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--profile", choices=("targeted", "full"), required=True)
    parser.add_argument("--output", type=Path, required=True, help="new output directory outside a source tree")
    args = parser.parse_args()
    output = args.output.resolve()
    if any(output == root or root in output.parents for root in [ROOT.resolve()]):
        parser.error("output must be outside the contributor source tree")
    if not output.parent.is_dir():
        parser.error("output parent directory must exist")
    surrounding_repo = subprocess.run(["git", "-C", str(output.parent), "rev-parse", "--show-toplevel"],
                                      capture_output=True, text=True)
    if surrounding_repo.returncode == 0:
        parser.error("output must be outside every Git worktree")
    try:
        signal.signal(signal.SIGINT, cancel)
        signal.signal(signal.SIGTERM, cancel)
        experiment = Experiment(args.manifest, output, args.profile)
        return experiment.run()
    except (OSError, ValueError) as error:
        print(f"experiment: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
