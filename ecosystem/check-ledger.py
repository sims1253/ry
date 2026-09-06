#!/usr/bin/env python3
"""Validate that a corpus ledger's summary blocks agree with
its findings array.

Asserts:
  - sum(classification.values()) == len(findings)
  - sum(audit_group_counts.values()) == len(findings)
  - every classification entry equals the actual label count (entries for
    categories no finding carries must be 0)
  - every audit_group_counts entry equals the actual audit group count,
    likewise

Comparing per key over the union of summary and observed labels rejects
what per-key loops over the observed labels alone miss: a negative entry
for an unseen category offset by a positive one preserves both the total
and every observed count.

Usage:
  python3 ecosystem/check-ledger.py docs/corpus/posit-0.9.0.json
"""

import json
import sys
from collections import Counter


def compare_summary(name: str, summary: dict, actual: Counter, errors: list) -> None:
    """Require every summary entry to equal its count computed from findings.

    The summary may carry zero-valued entries for categories no finding
    uses (the classification taxonomy is fixed), so maps are compared per
    key rather than for exact equality: any key present in the summary
    must hold its actual count (0 for unseen categories), and any key the
    findings carry must be present.
    """
    for key in sorted(set(summary) | set(actual), key=repr):
        summary_count = summary.get(key, 0)
        actual_count = actual[key]  # Counter: unseen keys count as 0.
        if summary_count != actual_count:
            errors.append(
                f"{name}[{key!r}] ({summary_count}) != actual count ({actual_count})"
            )


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

    # Check classification matches labels exactly
    label_counts = Counter(f.get("label") for f in findings)
    compare_summary("classification", classification, label_counts, errors)

    # Check audit group counts
    group_counts = data.get("audit_group_counts", {})
    group_sum = sum(group_counts.values())
    if group_sum != n:
        errors.append(
            f"audit_group_counts sum ({group_sum}) != findings length ({n})"
        )

    # Check audit group counts match exactly
    actual_groups = Counter(f.get("audit_group") for f in findings)
    compare_summary("audit_group_counts", group_counts, actual_groups, errors)

    if errors:
        print(f"FAIL: {path}")
        for e in errors:
            print(f"  {e}")
        return 1

    print(f"OK: {path} — {n} findings, classification and audit group sums match")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: check-ledger.py <ledger.json> [<ledger.json> ...]", file=sys.stderr)
        sys.exit(2)

    exit_code = 0
    for path in sys.argv[1:]:
        exit_code |= check_ledger(path)

    sys.exit(exit_code)
