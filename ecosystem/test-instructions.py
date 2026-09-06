#!/usr/bin/env python3
"""Counter parsing and comparison guards; no hardware counter required."""

import copy
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

import instructions


class CounterTests(unittest.TestCase):
    def test_perf_uses_actual_unscaled_counter(self):
        self.assertEqual(instructions.parse_perf("12345;;instructions:u;1000;100.00;;\n"), 12345)
        for text in ("<not counted>;;instructions:u;0;0.00", "123;;instructions:u;100;50.00", "123;;instructions:u", ""):
            with self.assertRaises(ValueError):
                instructions.parse_perf(text)

    def test_callgrind_selects_ir_event(self):
        self.assertEqual(instructions.parse_callgrind("events: Dr Ir Dw\ntotals: 10 12345 20\n"), 12345)
        for text in ("events: Dr\ntotals: 10", "events: Ir\nsummary: 0\n", "events: Ir\ntotals: 0"):
            with self.assertRaises(ValueError):
                instructions.parse_callgrind(text)

    @unittest.skipUnless(shutil.which("valgrind"), "Callgrind is not installed")
    def test_real_counter_and_timeout(self):
        env = dict(os.environ, **instructions.ENVIRONMENT)
        self.assertGreater(instructions.count("callgrind", [shutil.which("true")], Path.cwd(), 30, env), 0)
        with self.assertRaises(subprocess.TimeoutExpired):
            instructions.count("callgrind", [shutil.which("sleep"), "10"], Path.cwd(), 0.01, env)


class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.old = {"schema_version": 1, "source_revision": "a" * 40,
                    "metadata": {"backend": "callgrind", "architecture": "x86_64", "rustc": "fixed"},
                    "packages": [{"package": "glue", "revision": "b" * 40, "tree": "c" * 40,
                                  "url": "https://example.invalid/glue", "status": "measured", "instructions": 100}]}
        self.new = copy.deepcopy(self.old)

    def test_growth_warns_without_changing_counts(self):
        self.new["packages"][0]["instructions"] = 125
        report = instructions.compare(self.old, self.new, 10)
        self.assertIn("+25.00% | WARNING", report)

    def test_incompatible_backend_architecture_or_toolchain_has_no_delta(self):
        for field in self.old["metadata"]:
            changed = copy.deepcopy(self.new)
            changed["metadata"][field] = "different"
            report = instructions.compare(self.old, changed, 10)
            self.assertIn("INCOMPARABLE", report)
            self.assertNotIn("| Delta |", report)

    def test_changed_sample_is_not_a_regression(self):
        self.new["packages"][0]["revision"] = "d" * 40
        self.assertIn("INCOMPARABLE sample", instructions.compare(self.old, self.new, 10))

    def test_failed_and_missing_measurements_are_explicit(self):
        self.new["packages"][0] = {"package": "glue", "status": "unavailable", "reason": "timeout"}
        self.assertIn("UNAVAILABLE", instructions.compare(self.old, self.new, 10))
        self.new["packages"] = []
        self.assertIn("UNAVAILABLE current row", instructions.compare(self.old, self.new, 10))

    def test_invalid_counts_are_rejected(self):
        for count in (0, -1, "123", None, True, float("nan"), float("inf")):
            self.new["packages"][0]["instructions"] = count
            with self.assertRaises(ValueError):
                instructions.compare(self.old, self.new, 10)

    def test_failed_build_ledgers_need_no_revision_or_counts(self):
        failed = {"schema_version": 1, "metadata": {"backend": "unavailable"}, "packages": []}
        self.assertIn("UNAVAILABLE", instructions.compare(failed, failed, 10))


class ManifestFailureTests(unittest.TestCase):
    def test_invalid_manifest_still_writes_failure_ledger(self):
        valid = "glue https://example.invalid/glue " + "a" * 40 + "\n"
        for contents in ("glue https://example.invalid/glue not-a-pin\n", valid + valid, "glue missing-column\n"):
            with self.subTest(contents=contents), tempfile.TemporaryDirectory() as directory:
                manifest = Path(directory) / "packages.txt"
                output = Path(directory) / "ledger.json"
                manifest.write_text(contents)
                stderr = io.StringIO()
                with mock.patch.object(instructions, "MANIFEST", manifest), \
                     mock.patch.object(sys, "argv", ["instructions.py", "measure", "--output", str(output)]), \
                     mock.patch.object(instructions, "measure") as measure, contextlib.redirect_stderr(stderr):
                    self.assertEqual(instructions.main(), 1)
                measure.assert_not_called()
                ledger = json.loads(output.read_text())
                self.assertEqual(ledger["packages"], [])
                self.assertEqual(ledger["metadata"]["backend"], "unavailable")
                self.assertTrue(ledger["reason"])
                self.assertNotIn("Traceback", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
