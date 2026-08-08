#!/usr/bin/env python3
"""Generate/check readable Posit diagnostic messages keyed by ledger identity."""

from __future__ import annotations

import argparse
import copy
import difflib
import json
import sys
from pathlib import Path


def identity(finding: dict) -> str:
    return (
        f"{finding['package']}::{finding['code']}::{finding['path']}:"
        f"{finding['line']}:{finding['column']}"
    )


def rendered(document: dict) -> str:
    return json.dumps(document, ensure_ascii=False, indent=2, sort_keys=False) + "\n"


def relative_path_matches(actual: str, expected: str) -> bool:
    actual = actual.replace("\\", "/").removeprefix("./")
    expected = expected.replace("\\", "/").removeprefix("./")
    return actual == expected or actual.endswith("/" + expected)


def observed_entries(
    ledger: dict, json_dir: Path, report_prefix: str, packages: list[str]
) -> dict[str, dict]:
    expected = [row for row in ledger["findings"] if row["package"] in packages]
    by_package: dict[str, list[dict]] = {}
    for finding in expected:
        by_package.setdefault(finding["package"], []).append(finding)

    observed: dict[str, dict] = {}
    for package, findings in by_package.items():
        report = json_dir / f"{report_prefix}{package}.root.json"
        diagnostics = json.loads(report.read_text(encoding="utf-8"))
        for finding in findings:
            matches = [
                diagnostic
                for diagnostic in diagnostics
                if diagnostic["code"] == finding["code"]
                and diagnostic["line"] == finding["line"]
                and diagnostic["column"] == finding["column"]
                and relative_path_matches(diagnostic["path"], finding["path"])
            ]
            if len(matches) != 1:
                raise SystemExit(
                    f"posit messages: expected one production diagnostic for "
                    f"{identity(finding)}, found {len(matches)}"
                )
            diagnostic = matches[0]
            entry = {
                "package": finding["package"],
                "code": finding["code"],
                "path": finding["path"],
                "line": finding["line"],
                "column": finding["column"],
                "severity": diagnostic["severity"],
                "message": diagnostic["message"],
            }
            if "fix" in diagnostic:
                entry["fix"] = diagnostic["fix"]
            observed[identity(finding)] = entry
    return observed


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("check", "update"))
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--messages", type=Path, required=True)
    parser.add_argument("--json-dir", type=Path, required=True)
    parser.add_argument("--report-prefix", default="")
    parser.add_argument("packages", nargs="+")
    args = parser.parse_args()

    ledger = json.loads(args.ledger.read_text(encoding="utf-8"))
    observed = observed_entries(
        ledger, args.json_dir, args.report_prefix, args.packages
    )
    if args.messages.exists():
        committed = json.loads(args.messages.read_text(encoding="utf-8"))
    else:
        committed = {
            "schema_version": 1,
            "corpus": "posit",
            "ry_version": "0.9.0-dev",
            "identity_fields": ["package", "code", "path", "line", "column"],
            "findings": {},
        }
    candidate = copy.deepcopy(committed)
    candidate["findings"].update(observed)
    candidate["findings"] = dict(sorted(candidate["findings"].items()))

    if args.mode == "update":
        args.messages.write_text(rendered(candidate), encoding="utf-8")
        print(
            f"posit messages: updated {len(observed)} identities in {args.messages}"
        )
        return 0

    missing = [key for key in observed if key not in committed["findings"]]
    stale = [
        key
        for key, value in observed.items()
        if committed["findings"].get(key) != value
    ]
    if not missing and not stale:
        print(f"posit messages: {len(observed)} readable identities match")
        return 0

    before = rendered(committed).splitlines(keepends=True)
    after = rendered(candidate).splitlines(keepends=True)
    sys.stderr.writelines(
        difflib.unified_diff(
            before,
            after,
            fromfile=str(args.messages),
            tofile=f"{args.messages} (production)",
            n=8,
        )
    )
    print(
        f"posit messages: drift in {len(set(missing + stale))} stable identities",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
