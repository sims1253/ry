#!/usr/bin/env python3
"""Controls for the experiment report's evidence and failure states."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import experiment_report as report


def finding(code, line, column=1):
    return {"code": code, "line": line, "column": column}


class FindingDiffTests(unittest.TestCase):
    def test_historical_324_shape_exposes_lost_true_positive(self):
        # Models the failure shape; this does not rerun the historical experiment.
        retained = finding("RY010", 3)
        lost_tp = finding("RY040", 8)
        replacement = finding("RY041", 12)
        fixture = {"expected_additions": [replacement], "expected_removals": [lost_tp]}
        before = [retained, lost_tp]
        after = [retained, replacement]
        added, removed, state, reason = report.evaluate_fixture(
            fixture, before, after, {report.identity(lost_tp): "true_positive"})
        self.assertEqual(len(before), len(after))
        self.assertEqual(added, [{**replacement, "label": "unreviewed"}])
        self.assertEqual(removed, [{**lost_tp, "label": "true_positive"}])
        self.assertEqual((state, reason), ("failed", "reviewed true positive removed"))

    def test_expected_added_finding_and_quiet_control(self):
        new = finding("RY106", 5)
        self.assertEqual(report.evaluate_fixture({"expected_additions": [new]}, [], [new], {})[2], "passed")
        self.assertEqual(report.evaluate_fixture({}, [], [], {})[2], "passed")
        self.assertEqual(report.evaluate_fixture({}, [], [new], {})[2], "failed")

    def test_triage_label_requires_matching_pinned_package(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory)
            ledger = source / "ledger.json"
            ledger.write_text(json.dumps({"packages": [{"name": "pkg", "commit": "a" * 40}],
                                          "findings": [{"package": "pkg", "path": "R/file.R",
                                                        "code": "RY040", "line": 8, "column": 1,
                                                        "label": "true_positive"}]}))
            fixture = {"path": "R/file.R", "workload": {"revision": "a" * 40},
                       "triage": {"ledger": "ledger.json", "package": "pkg", "path": "R/file.R"}}
            labels, digest = report.labels_for(fixture, source)
            self.assertEqual(labels[("RY040", 8, 1)], "true_positive")
            self.assertEqual(digest, report.sha256(ledger))
            fixture["workload"]["revision"] = "b" * 40
            with self.assertRaisesRegex(ValueError, "pin differs"):
                report.labels_for(fixture, source)
            ledger.write_text(json.dumps({"packages": [{"name": "pkg"}], "findings": []}))
            with self.assertRaisesRegex(ValueError, "malformed triage package"):
                report.labels_for(fixture, source)


class InstructionStatusTests(unittest.TestCase):
    def setUp(self):
        self.old = {"schema_version": 1, "metadata": {"backend": "callgrind", "rustc": "1.98.1"},
                    "packages": [{"package": "one", "revision": "a" * 40, "tree": "b" * 40,
                                  "url": "https://example.invalid/one", "status": "measured", "instructions": 123}]}
        self.new = json.loads(json.dumps(self.old))

    def test_changed_workload_pin_is_incomparable(self):
        self.new["packages"][0]["revision"] = "c" * 40
        self.assertEqual(report.compare_instructions(self.old, self.new)[0], "incomparable")

    def test_missing_counter_is_unavailable(self):
        self.new["metadata"]["backend"] = "unavailable"
        self.assertEqual(report.compare_instructions(self.old, self.new)[0], "unavailable")

    def test_same_metadata_is_comparable_without_claiming_exact_repetition(self):
        self.new["packages"][0]["instructions"] = 128
        self.assertEqual(report.compare_instructions(self.old, self.new)[0], "passed")


class RunStatusTests(unittest.TestCase):
    def make_experiment(self, root):
        manifest = {"schema_version": 1, "hypothesis": "one", "expected_change": "two",
                    "reference": "HEAD~1", "candidate": "HEAD", "invariants": ["no lost TP"],
                    "fixtures": [{"name": "witness", "role": "witness", "path": "witness.R"},
                                 {"name": "quiet", "role": "quiet_control", "path": "quiet.R"}]}
        path = root / "manifest.json"
        path.write_text(json.dumps(manifest))
        return report.Experiment(path, root / "out", "full")

    def test_failed_subprocess_stays_failed_with_raw_output(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            row = experiment.command("failure", ["python3", "-c", "import sys; print('bad'); sys.exit(7)"], Path(directory))
            self.assertEqual((row["status"], row["exit_code"]), ("failed", 7))
            self.assertEqual(Path(row["stdout"]).read_text(), "bad\n")
            self.assertEqual(experiment.finish(), 1)
            self.assertFalse(json.loads((experiment.output / "report.json").read_text())["fully_validated"])

    def test_nonexecutable_command_records_unavailable_stage(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(report.subprocess, "Popen", side_effect=PermissionError("not executable")):
                row = experiment.command("blocked", ["/path/to/tool"], Path(directory))
            self.assertEqual(row["status"], "unavailable")
            self.assertIn("not executable", row["reason"])
            experiment.finish()
            self.assertTrue((experiment.output / "report.json").exists())

    def test_child_exit_during_cancel_keeps_cancelled_stage(self):
        class ExitedChild:
            pid = 123456

            def communicate(self, timeout=None):
                if timeout is not None:
                    raise report.Cancelled("interrupted")
                return b"", b""

        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(report.subprocess, "Popen", return_value=ExitedChild()), \
                 mock.patch.object(report.os, "killpg", side_effect=ProcessLookupError):
                with self.assertRaises(report.Cancelled):
                    experiment.command("cancelled", ["python3"], Path(directory))
            stage = experiment.report["stages"][-1]
            self.assertEqual(stage["status"], "cancelled")
            self.assertEqual(Path(stage["stdout"]).read_bytes(), b"")
            self.assertEqual(Path(stage["stderr"]).read_bytes(), b"")
            experiment.finish()
            self.assertEqual(experiment.report["status"], "cancelled")
            saved = json.loads((experiment.output / "report.json").read_text())
            self.assertEqual(saved["status"], "cancelled")

    def test_child_exit_during_timeout_remains_failed(self):
        class ExitedChild:
            pid = 123456

            def communicate(self, timeout=None):
                if timeout is not None:
                    raise subprocess.TimeoutExpired("probe", timeout)
                return b"partial", b"timeout"

        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(report.subprocess, "Popen", return_value=ExitedChild()), \
                 mock.patch.object(report.os, "killpg", side_effect=ProcessLookupError):
                row = experiment.command("timed-out", ["python3"], Path(directory), timeout=1)
            self.assertEqual(row["status"], "failed")
            self.assertEqual(Path(row["stdout"]).read_bytes(), b"partial")
            self.assertEqual(Path(row["stderr"]).read_bytes(), b"timeout")
            experiment.finish()
            self.assertEqual(experiment.report["status"], "failed")

    def test_instruction_adapter_does_not_inject_distinct_cargo_target_dir(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            row = experiment.command("probe", ["python3", "-c", "import os; print(os.getenv('CARGO_TARGET_DIR', 'unset'))"],
                                     Path(directory), cargo_target=False)
            self.assertEqual(row["status"], "passed")
            self.assertEqual(Path(row["stdout"]).read_text(), "unset\n")
            self.assertNotIn("CARGO_TARGET_DIR", row["environment"])

    def test_missing_r_is_unavailable_and_full_never_validated(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(report.shutil, "which", return_value=None):
                row = experiment.command("oracle", ["cargo", "test"], Path(directory), requires_r=True)
            self.assertEqual(row["status"], "unavailable")
            experiment.finish()
            self.assertEqual(experiment.report["status"], "unavailable")
            self.assertFalse(experiment.report["fully_validated"])

    def test_cancellation_is_recorded_in_both_reports(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(experiment, "setup", side_effect=report.Cancelled("interrupted")):
                self.assertEqual(experiment.run(), 1)
            self.assertEqual(experiment.report["status"], "cancelled")
            self.assertIn("cancelled", (experiment.output / "report.md").read_text())

    def test_targeted_can_pass_but_never_claim_full_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.profile = "targeted"
            experiment.stage("fixture-diff-witness", "passed")
            experiment.stage("fixture-diff-quiet", "passed")
            experiment.stage("instructions", "skipped", reason="not selected")
            self.assertEqual(experiment.finish(), 0)
            self.assertFalse(experiment.report["fully_validated"])

    def test_full_validation_requires_ordered_gates_and_comparable_counter(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            for name, _, _ in report.GATES:
                experiment.stage(name, "passed")
            experiment.stage("fixture-diff-witness", "passed")
            experiment.stage("fixture-diff-quiet", "passed")
            experiment.stage("instructions", "passed")
            self.assertEqual(experiment.finish(), 0)
            self.assertTrue(experiment.report["fully_validated"])
            experiment.report["stages"][-1]["status"] = "incomparable"
            self.assertEqual(experiment.finish(), 1)
            self.assertFalse(experiment.report["fully_validated"])

    def test_changed_instruction_sample_pin_is_incomparable_before_measurement(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.manifest["instructions"] = {"enabled": True}
            for side, content in (("reference", "old"), ("candidate", "new")):
                ecosystem = experiment.output / side / "ecosystem"
                ecosystem.mkdir(parents=True)
                (ecosystem / "instruction-packages.txt").write_text(content)
            (experiment.output / "candidate" / "ecosystem" / "instructions.py").write_text("harness")
            experiment.run_instructions()
            self.assertEqual(experiment.report["stages"][-1]["status"], "incomparable")
            experiment.finish()
            self.assertFalse(experiment.report["fully_validated"])

    def prepare_instruction_files(self, experiment):
        for side in ("reference", "candidate"):
            ecosystem = experiment.output / side / "ecosystem"
            ecosystem.mkdir(parents=True)
            (ecosystem / "instruction-packages.txt").write_text("same pins")
        (experiment.output / "candidate" / "ecosystem" / "instructions.py").write_text("harness")

    def test_missing_ledger_after_successful_measure_has_failed_stage_and_report(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.manifest["instructions"] = {"enabled": True}
            self.prepare_instruction_files(experiment)
            with mock.patch.object(experiment, "command", side_effect=lambda name, *args, **kwargs: experiment.stage(name, "passed")):
                experiment.run_instructions()
            self.assertEqual(experiment.report["stages"][-1]["status"], "failed")
            self.assertIn("without ledger", experiment.report["stages"][-1]["reason"])
            self.assertEqual(experiment.finish(), 1)
            self.assertTrue((experiment.output / "report.json").exists())

    def test_malformed_measurement_ledger_has_failed_stage_and_report(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.manifest["instructions"] = {"enabled": True}
            self.prepare_instruction_files(experiment)
            (experiment.output / "instructions-reference.json").write_text("not JSON")
            with mock.patch.object(experiment, "command", side_effect=lambda name, *args, **kwargs: experiment.stage(name, "passed")):
                experiment.run_instructions()
            self.assertEqual(experiment.report["stages"][-1]["status"], "failed")
            self.assertIn("ledger missing or invalid", experiment.report["stages"][-1]["reason"])
            experiment.finish()
            self.assertTrue((experiment.output / "report.md").exists())

    def test_unavailable_counter_keeps_raw_failed_exit_and_layer_status(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.manifest["instructions"] = {"enabled": True}
            self.prepare_instruction_files(experiment)
            (experiment.output / "instructions-reference.json").write_text(json.dumps({
                "schema_version": 1, "metadata": {"backend": "unavailable"},
                "reason": "No usable instruction counter", "packages": []}))
            with mock.patch.object(experiment, "command", side_effect=lambda name, *args, **kwargs:
                                   experiment.stage(name, "failed", exit_code=1)):
                experiment.run_instructions()
            self.assertEqual(experiment.report["stages"][0]["status"], "failed")
            self.assertEqual(experiment.report["stages"][0]["exit_code"], 1)
            self.assertEqual(experiment.report["stages"][1]["status"], "unavailable")
            experiment.finish()
            self.assertFalse(experiment.report["fully_validated"])

    def test_failed_build_marks_every_selected_fixture_skipped(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            with mock.patch.object(experiment, "command", side_effect=lambda name, *args, **kwargs:
                                   experiment.stage(name, "failed", exit_code=42)):
                experiment.run_fixtures()
            self.assertEqual([item["status"] for item in experiment.report["fixtures"]], ["skipped", "skipped"])
            self.assertEqual([row["status"] for row in experiment.report["stages"] if row["name"].startswith("fixture-diff-")],
                             ["skipped", "skipped"])

    def test_malformed_triage_ledger_finalizes_with_later_fixture_skipped(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            fixture = experiment.manifest["fixtures"][0]
            fixture["workload"] = {"repository": str(Path(directory)), "revision": "a" * 40}
            fixture["triage"] = {"ledger": "ledger.json", "package": "pkg", "path": fixture["path"]}
            candidate = experiment.output / "candidate"
            candidate.mkdir()
            (candidate / "ledger.json").write_text("{}")
            for side in ("reference", "candidate"):
                binary = experiment.output / f"target-{side}" / "debug" / "ry"
                binary.parent.mkdir(parents=True)
                binary.write_bytes(b"binary")
                experiment.report["metadata"][side] = {}
            with mock.patch.object(experiment, "command", side_effect=lambda name, *args, **kwargs: experiment.stage(name, "passed")):
                experiment.run_fixtures()
            self.assertEqual(experiment.report["fixtures"][0]["status"], "failed")
            self.assertEqual(experiment.report["fixtures"][1]["status"], "skipped")
            self.assertEqual([row["status"] for row in experiment.report["stages"] if row["name"].startswith("fixture-diff-")],
                             ["skipped", "skipped"])
            self.assertEqual(experiment.finish(), 1)
            self.assertTrue((experiment.output / "report.json").exists())

    def test_malformed_manifest_finding_and_triage_fail_cleanly(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            fixture = experiment.manifest["fixtures"][0]
            fixture["expected_additions"] = [{"code": "RY106"}]
            with self.assertRaisesRegex(ValueError, "identity needs"):
                report.validate_manifest(experiment.manifest)
            fixture["expected_additions"] = []
            fixture["triage"] = {"package": "pkg", "path": fixture["path"]}
            with self.assertRaisesRegex(ValueError, "repository-relative file"):
                report.validate_manifest(experiment.manifest)

            path = Path(directory) / "bad.json"
            path.write_text(json.dumps(experiment.manifest))
            result = subprocess.run([sys.executable, str(Path(report.__file__)), str(path),
                                     "--profile", "targeted", "--output", str(Path(directory) / "unused")],
                                    text=True, capture_output=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn("experiment:", result.stderr)
            self.assertNotIn("Traceback", result.stderr)

    def test_nonobject_manifest_and_nonstring_workload_revision_exit_two(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            experiment = self.make_experiment(root)
            invalid = [[], None]
            with_bad_workload = json.loads(json.dumps(experiment.manifest))
            with_bad_workload["fixtures"][0]["workload"] = {
                "repository": str(root), "revision": 42}
            invalid.append(with_bad_workload)
            for index, manifest in enumerate(invalid):
                path = root / f"invalid-{index}.json"
                path.write_text(json.dumps(manifest))
                result = subprocess.run([sys.executable, str(Path(report.__file__)), str(path),
                                         "--profile", "targeted", "--output", str(root / f"unused-{index}")],
                                        text=True, capture_output=True)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertIn("experiment:", result.stderr)
                self.assertNotIn("Traceback", result.stderr)

    def test_timed_out_r_package_metadata_stays_in_report(self):
        with tempfile.TemporaryDirectory() as directory:
            experiment = self.make_experiment(Path(directory))
            experiment.manifest["r_packages"] = ["rlang"]
            def probe(argv, **_kwargs):
                if argv[:2] == ["Rscript", "-e"]:
                    raise subprocess.TimeoutExpired(argv, 30)
                return mock.Mock(returncode=0, stdout="version", stderr="")
            with mock.patch.object(report.shutil, "which", side_effect=lambda name, **kwargs: name), \
                 mock.patch.object(report.subprocess, "run", side_effect=probe):
                experiment.record_metadata()
            self.assertEqual(experiment.report["stages"][-1]["status"], "unavailable")
            self.assertIn("timed out", experiment.report["stages"][-1]["reason"])
            experiment.finish()
            self.assertTrue((experiment.output / "report.json").exists())


if __name__ == "__main__":
    unittest.main()
