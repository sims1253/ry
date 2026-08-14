#!/usr/bin/env python3
"""P37-W5: Validate that a corpus ledger's summary blocks agree with
its findings array.

Asserts:
  - sum(classification.values()) == len(findings)
  - sum(workstream_counts.values()) == len(findings)
  - classification counts match the labels in findings
  - workstream_counts match the workstream field in findings

Usage:
  python3 ecosystem/check-ledger.py docs/corpus/posit-0.9.0.json
"""

import json
import sys
from collections import Counter


def check_ledger(path: str) -> int:
    with open(path) as f:
        data = json.load(f)

    findings = data.get("findings", [])
    n = len(findings)

    errors = []

    # Check classification summary
    classification = data.get("classification", {})
    cls_sum = sum(classification.values())
    if cls_sum != n:
        errors.append(
            f"classification sum ({cls_sum}) != findings length ({n})"
        )

    # Check classification matches labels
    label_counts = Counter(f.get("label") for f in findings)
    for label, count in label_counts.items():
        summary_count = classification.get(label, 0)
        if count != summary_count:
            errors.append(
                f"classification[{label}] ({summary_count}) != "
                f"actual label count ({count})"
            )

    # Check workstream counts
    ws_counts = data.get("workstream_counts", {})
    ws_sum = sum(ws_counts.values())
    if ws_sum != n:
        errors.append(
            f"workstream_counts sum ({ws_sum}) != findings length ({n})"
        )

    # Check workstream counts match
    actual_ws = Counter(f.get("workstream") for f in findings)
    for ws, count in actual_ws.items():
        summary_count = ws_counts.get(ws, 0)
        if count != summary_count:
            errors.append(
                f"workstream_counts[{ws}] ({summary_count}) != "
                f"actual count ({count})"
            )

    if errors:
        print(f"FAIL: {path}")
        for e in errors:
            print(f"  {e}")
        return 1

    print(f"OK: {path} — {n} findings, classification and workstream sums match")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: check-ledger.py <ledger.json> [<ledger.json> ...]", file=sys.stderr)
        sys.exit(2)

    exit_code = 0
    for path in sys.argv[1:]:
        exit_code |= check_ledger(path)

    sys.exit(exit_code)
