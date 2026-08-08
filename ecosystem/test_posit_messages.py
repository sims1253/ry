#!/usr/bin/env python3
import json
import subprocess
import tempfile
import unittest
from pathlib import Path


class PositMessagesGateTest(unittest.TestCase):
    def test_one_word_message_change_has_readable_unified_diff(self):
        root = Path(__file__).resolve().parents[1]
        script = root / "ecosystem/posit_messages.py"
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            ledger = temp / "ledger.json"
            messages = temp / "messages.json"
            report = temp / "posit.demo.root.json"
            ledger.write_text(json.dumps({
                "findings": [{
                    "package": "demo", "code": "RY010", "path": "R/x.R",
                    "line": 1, "column": 1,
                }]
            }))
            report.write_text(json.dumps([{
                "path": "/cache/demo/R/x.R", "code": "RY010",
                "line": 1, "column": 1, "severity": "warning",
                "message": "variable `old` is not bound",
            }]))
            base = [
                str(script), "update", "--ledger", str(ledger),
                "--messages", str(messages), "--json-dir", str(temp),
                "--report-prefix", "posit.", "demo",
            ]
            subprocess.run(base, check=True, capture_output=True, text=True)
            document = json.loads(messages.read_text())
            entry = next(iter(document["findings"].values()))
            entry["message"] = "variable `new` is not bound"
            messages.write_text(json.dumps(document, indent=2) + "\n")

            result = subprocess.run(
                [base[0], "check", *base[2:]],
                check=False,
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('-      "message": "variable `new` is not bound"', result.stderr)
            self.assertIn('+      "message": "variable `old` is not bound"', result.stderr)
            self.assertIn("demo::RY010::R/x.R:1:1", result.stderr)


if __name__ == "__main__":
    unittest.main()
