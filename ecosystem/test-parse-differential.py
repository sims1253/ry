#!/usr/bin/env python3
"""The parse differential fails on a new false RY000 and on newly accepted
R-rejected input, and ignores R message and note changes. Needs Rscript."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().with_name("parse_differential.py")

# A stand-in for `ry check --output-format json`: RY000 on every listed file
# whose name is in $FAKE_RY000.
FAKE_RY = """#!/usr/bin/env python3
import json, os, sys
rejected = set(filter(None, os.environ.get("FAKE_RY000", "").split(",")))
paths = sys.argv[sys.argv.index("--") + 1:]
print(json.dumps([
    {"path": path, "code": "RY000", "line": 1, "column": 1}
    for path in paths if os.path.basename(path) in rejected
]))
"""


class ParseDifferentialGateTest(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp())
        self.addCleanup(subprocess.run, ["rm", "-rf", str(self.temp)], check=True)
        source = self.temp / "demo"
        (source / "R").mkdir(parents=True)
        (source / "R" / "valid.R").write_text("x <- 1\n")
        (source / "R" / "invalid.R").write_text("x <- )\n")
        self.binary = self.temp / "ry"
        self.binary.write_text(FAKE_RY)
        self.binary.chmod(0o755)
        self.ledger = self.temp / "ledger.json"
        self.source = f"demo={source}"

    def run_gate(self, mode, rejected):
        return subprocess.run(
            [str(SCRIPT), mode, "--binary", str(self.binary), "--ledger",
             str(self.ledger), "--corpus", "demo", self.source],
            env={**os.environ, "FAKE_RY000": rejected},
            capture_output=True,
            text=True,
            check=False,
        )

    def test_agreement_is_recorded_and_checks(self):
        self.assertEqual(self.run_gate("update", "invalid.R").returncode, 0)
        corpus = json.loads(self.ledger.read_text())["corpora"]["demo"]
        self.assertEqual(
            (corpus["files"], corpus["r_rejected"], corpus["ry_rejected"],
             corpus["disagreements"]),
            (2, 1, 1, 0),
        )
        self.assertEqual(self.run_gate("check", "invalid.R").returncode, 0)

    def test_false_ry000_on_valid_r_fails(self):
        self.run_gate("update", "invalid.R")
        result = self.run_gate("check", "invalid.R,valid.R")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('"path": "R/valid.R"', result.stderr)
        self.assertIn('"ry": "reject"', result.stderr)

    def test_newly_accepted_r_rejected_input_fails(self):
        self.run_gate("update", "invalid.R")
        result = self.run_gate("check", "")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('"path": "R/invalid.R"', result.stderr)
        self.assertIn('"ry": "accept"', result.stderr)

    def test_known_disagreement_keeps_its_note_and_ignores_messages(self):
        self.run_gate("update", "")
        document = json.loads(self.ledger.read_text())
        [entry] = document["corpora"]["demo"]["disagreement_files"]
        self.assertEqual((entry["r"], entry["ry"]), ("reject", "accept"))
        entry["note"] = "known: tracked elsewhere"
        entry["r_message"] = "a message from another R version"
        self.ledger.write_text(json.dumps(document, indent=2) + "\n")
        self.assertEqual(self.run_gate("check", "").returncode, 0)
        self.run_gate("update", "")
        [entry] = json.loads(self.ledger.read_text())["corpora"]["demo"]["disagreement_files"]
        self.assertEqual(entry["note"], "known: tracked elsewhere")
        # Fixing the disagreement changes the ratchet: regenerate to tighten.
        self.assertNotEqual(self.run_gate("check", "invalid.R").returncode, 0)


if __name__ == "__main__":
    unittest.main()
