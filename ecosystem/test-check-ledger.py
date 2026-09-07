#!/usr/bin/env python3
import copy
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class LedgerPackageCountsTest(unittest.TestCase):
    def setUp(self):
        self.ledger = {
            "packages": [
                {"name": "alpha", "diagnostics": 2},
                {"name": "beta", "diagnostics": 1},
                {"name": "empty", "diagnostics": 0},
            ],
            "findings": [
                {"package": package, "label": "false_positive", "audit_group": "reviewed"}
                for package in ["alpha", "alpha", "beta"]
            ],
            "classification": {"false_positive": 3},
            "audit_group_counts": {"reviewed": 3},
        }

    def check(self, ledger):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "ledger.json"
            path.write_text(json.dumps(ledger))
            return subprocess.run(
                [sys.executable, str(Path(__file__).with_name("check-ledger.py")), str(path)],
                capture_output=True, text=True, check=False,
            )

    def test_matching_counts_include_repeated_findings_and_zero_packages(self):
        result = self.check(self.ledger)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_stale_package_counts_fail_even_if_the_total_is_unchanged(self):
        for counts in [(3, 1, 0), (2, 0, 0), (2, 1, 1), (1, 2, 0)]:
            with self.subTest(counts=counts):
                ledger = copy.deepcopy(self.ledger)
                for package, count in zip(ledger["packages"], counts):
                    package["diagnostics"] = count
                result = self.check(ledger)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("packages[", result.stdout)
                self.assertIn("!= actual count", result.stdout)

    def test_historical_ledgers_can_omit_package_counts(self):
        for package in self.ledger["packages"]:
            package.pop("diagnostics")
        result = self.check(self.ledger)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_a_partly_populated_summary_still_validates_declared_counts(self):
        self.ledger["packages"][0].pop("diagnostics")
        self.ledger["packages"][1]["diagnostics"] = 7
        result = self.check(self.ledger)
        self.assertEqual(result.returncode, 1)
        self.assertIn("packages['beta'].diagnostics (7) != actual count (1)", result.stdout)


if __name__ == "__main__":
    unittest.main()
