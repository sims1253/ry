#!/usr/bin/env python3
"""Transcribe the ry 0.8.0 Posit-corpus audit into docs/corpus/posit-0.8.0.json.

This is the *reproducible* generator for the Posit open-source ledger. Rather
than hand-writing 1,142 finding rows, it reads the audited artefacts and joins
each `ry` diagnostic identity to its independent classification, then assigns a
deterministic workstream label from the auditor's rationale.

Inputs (under ``--audit-dir``, the ry-audits/posit-corpus checkout):

  packages.json                      slug -> {repo, stars}
  aggregate.json                     validated headline numbers + sha256 anchor
  audit-results/<slug>/ry-stdout.json  raw ry diagnostics (code/path/line/column)
  audit-results/<slug>/summary.json    auditor classification (TP/FP/uncertain)
  audit-results/<slug>/git_commit      pinned commit used for the audit

For every diagnostic in ``ry-stdout.json`` the script looks up the matching
classification entry in ``summary.json`` (joined on code, relative path, line
and message). Every diagnostic MUST resolve to exactly one label; an orphan or
a conflicting duplicate label aborts transcription, so a package can never be
silently dropped or mis-counted.

Invariants asserted before writing (these are the "34 TP / 1108 FP, no silent
skips" guarantees):

  * 62 packages present, all with ry-stdout.json + summary.json
  * TP + FP + uncertain == total ry diagnostics == 1,142
  * exactly 34 true positives and 1,108 false positives, 0 uncertain
  * every ry diagnostic identity maps to exactly one classified row
  * classification_counts in every summary.json equals its array lengths
  * the joined identity multiset is free of conflicting labels

Run::

    ecosystem/transcribe-posit-corpus.py            # uses the default audit dir
    ecosystem/transcribe-posit-corpus.py --check    # validate an existing corpus
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from collections import Counter, defaultdict
from pathlib import Path

# --- expected invariants --------------------------------------------------
EXPECTED_PACKAGES = 62
EXPECTED_DIAGNOSTICS = 1142
EXPECTED_TP = 34
EXPECTED_FP = 1108
EXPECTED_UNCERTAIN = 0

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_AUDIT_DIR = Path(os.environ.get(
    "RY_POSIT_AUDIT_DIR", REPO_ROOT.parent / "ry-audits" / "posit-corpus"))
DEFAULT_OUTPUT = REPO_ROOT / "docs" / "corpus" / "posit-0.8.0.json"
RY_VERSION = "0.8.0"
RY_COMMIT = "da7261cfebafa2163546fd011743b4a06d47afb1"  # the v0.8.0 tag


def relative_package_path(abs_path: str) -> str:
    """Strip the audit clone root so paths are stable across machines.

    ry emits absolute paths like
    ``.../repos/tidyverse__dplyr/R/conditions.R``; the corpus stores paths
    relative to the package root (``R/conditions.R``), matching the tidyverse
    ledger and the auditor's ``summary.json`` ``file`` field.
    """
    marker = "/repos/"
    idx = abs_path.find(marker)
    if idx >= 0:
        rest = abs_path[idx + len(marker):]
        slash = rest.find("/")
        if slash >= 0:
            return rest[slash + 1:]
    return abs_path


def classify_workstream(code: str, label: str, rationale: str) -> str:
    """Assign a deterministic workstream slug from rule + rationale.

    Workstreams describe the *root cause* ry would address (for false
    positives) or where the defect lives (for true positives). The order of the
    checks is intentional: the most specific signals win, and the final
    ``type-narrowing`` bucket is the catch-all for legitimate runtime scalars ry
    over-narrowed. This mirrors the documented root-cause analysis in the audit
    SUMMARY.md (test fixtures, R6 portable=FALSE, local() closures, on_load,
    data-masking, native-routine registration, delayedAssign, S3 dispatch, the
    numeric->logical ``if`` idiom, and generic type over-narrowing).
    """
    r = rationale.lower()
    if label == "true_positive":
        # A real defect in the upstream package source, not in ry.
        return "upstream-package"

    if any(k in r for k in (
            "r6class", "portable = false", "portable=false", "r6 class",
            "public/private field", "r6 portable", "active binding of an r6")):
        return "r6-portable-false"
    if any(k in r for k in (
            "test fixture", "test scaffolding", "deliberate error fixture",
            "expect_error", "expect_snapshot", "deliberately malformed",
            "reformatted", "never executed", "never evaluated",
            "dummy_package", "bad-code", "bad code example", "intentional bad",
            "lintr test", "fixture parsed as data", "htmltemplate",
            "app template", "template (in inst", "test data",
            "parsed as data", "cascade artifact", "recovered-tree",
            "recovered tree", "parse recovery",
            "input to be reformatted", "deliberate test code",
            "deliberately triggers", "deliberately invalid",
            "intentionally triggers", "intentionally raises")):
        return "test-fixture"
    if any(k in r for k in (
            "local(", "local {", "local() closure", "local(...) closure",
            "local {...} block", "forward reference", "forward ref")):
        return "local-closure-forward-ref"
    if any(k in r for k in (
            "on_load", "on_load(", "on_load {", "namespace deferred",
            "executed at package load", "bound at package load",
            "namespace binding", "package-load", "package load")):
        return "namespace-on-load"
    if any(k in r for k in (
            "native routine", "usedynlib", "registered native", "c routine",
            "callentries", ".registration=true", ".registration = true",
            "registered in src", "ffi_", "call_with_c",
            "registered via useDynLib".replace("useDynLib", "usedynlib"),
            "src/init.c")):
        return "native-routine"
    if "delayedassign" in r:
        return "delayed-assign"
    if any(k in r for k in (
            "data-mask", "data mask", "data-masking", "nse",
            "column of", "bare column", "data-frame column",
            "data frame column", "masked column", "resolves to a column",
            "resolves to data", "data-masked")):
        return "data-masking-nse"
    if any(k in r for k in (
            "shadow", "shadows base", "shadowing base", "local closure")):
        return "function-shadowing"
    if any(k in r for k in (
            "s3 method", "s3 dispatch", "s3 generic", "+.gg", "+.glue",
            "dispatches to the s3", "the s3 method", "method dispatch",
            "s3 generic instead")):
        return "s3-method-dispatch"
    if any(k in r for k in (
            "<<-", "reassigned via", "reassigned to", "textconnection",
            "bound in the true-branch", "bound unconditionally",
            "set via <<-", "assigned in the true-branch",
            "reassigned by", "reassigned before")):
        return "reassign-scope"
    if code in ("RY001", "RY003") and any(k in r for k in (
            "length(", "nchar(", "sum(", "nzchar(", "length-1 integer",
            "length-1 numeric", "non-negative integer", "coerces nonzero",
            "idiom", "truthiness", "numeric-to-logical")):
        return "numeric-if-idiom"
    return "type-narrowing"


def load_audit(audit_dir: Path):
    if not audit_dir.is_dir():
        sys.exit(f"transcribe: audit dir not found: {audit_dir}")
    packages_meta = {p["slug"]: p for p in json.loads(
        (audit_dir / "packages.json").read_text())}
    aggregate = json.loads((audit_dir / "aggregate.json").read_text())
    audit_results = audit_dir / "audit-results"
    slugs = sorted(d.name for d in audit_results.iterdir() if d.is_dir())
    missing = sorted(set(packages_meta) - set(slugs))
    unexpected = sorted(set(slugs) - set(packages_meta))
    if missing or unexpected:
        sys.exit(
            "transcribe: package metadata/audit-results mismatch: "
            f"missing audit results={missing}; unexpected audit results={unexpected}"
        )
    return packages_meta, aggregate, audit_results, slugs


def build_findings(audit_results: Path, slugs):
    """Join ry diagnostics to classifications; assert no orphans/skips."""
    findings = []
    problems = []
    for slug in slugs:
        sdir = audit_results / slug
        ry = json.loads((sdir / "ry-stdout.json").read_text())
        summary = json.loads((sdir / "summary.json").read_text())

        # Validate the per-package summary is self-consistent.
        cc = summary["classification_counts"]
        arrays = {
            "true_positive": summary.get("true_positives", []),
            "false_positive": summary.get("false_positives", []),
            "uncertain": summary.get("uncertain", []),
        }
        for key, arr in arrays.items():
            if cc[key] != len(arr):
                problems.append(
                    f"{slug}: classification_counts.{key}={cc[key]} "
                    f"!= len({key})={len(arr)}")
        dc_total = summary["diagnostic_counts"].get("total", 0)
        if dc_total != len(ry):
            problems.append(
                f"{slug}: diagnostic_counts.total={dc_total} "
                f"!= ry-stdout diagnostics={len(ry)}")
        if sum(len(a) for a in arrays.values()) != dc_total:
            problems.append(
                f"{slug}: classified {sum(len(a) for a in arrays.values())} "
                f"!= diagnostic_counts.total={dc_total}")

        # Build the classification lookup keyed on the auditor's identity.
        lookup = {}
        for label, arr in arrays.items():
            for entry in arr:
                key = (entry["code"], entry["file"], str(entry["line"]),
                       entry["message"])
                if key in lookup and lookup[key] != label:
                    problems.append(
                        f"{slug}: conflicting labels for {key}: "
                        f"{lookup[key]} vs {label}")
                lookup[key] = label

        # Join every ry diagnostic to its classification.
        for diag in ry:
            rel = relative_package_path(diag["path"])
            key = (diag["code"], rel, str(diag["line"]), diag["message"])
            if key not in lookup:
                problems.append(
                    f"{slug}: orphan diagnostic {diag['code']} "
                    f"{rel}:{diag['line']} ({diag['message']!r})")
                continue
            label = lookup[key]
            rationale = ""
            for arr in arrays.values():
                for entry in arr:
                    if (entry["code"], entry["file"], str(entry["line"]),
                            entry["message"]) == key:
                        rationale = entry.get("rationale", "")
                        break
            findings.append({
                "package": slug,
                "code": diag["code"],
                "path": rel,
                "line": diag["line"],
                "column": diag["column"],
                "label": label,
                "workstream": classify_workstream(
                    diag["code"], label, rationale),
            })

    return findings, problems


def assert_invariants(slugs, findings, aggregate):
    failures = []
    if len(slugs) != EXPECTED_PACKAGES:
        failures.append(f"package count {len(slugs)} != {EXPECTED_PACKAGES}")
    counts = Counter(f["label"] for f in findings)
    tp = counts.get("true_positive", 0)
    fp = counts.get("false_positive", 0)
    unc = counts.get("uncertain", 0)
    if len(findings) != EXPECTED_DIAGNOSTICS:
        failures.append(
            f"finding count {len(findings)} != {EXPECTED_DIAGNOSTICS}")
    if tp != EXPECTED_TP:
        failures.append(f"true_positive {tp} != {EXPECTED_TP}")
    if fp != EXPECTED_FP:
        failures.append(f"false_positive {fp} != {EXPECTED_FP}")
    if unc != EXPECTED_UNCERTAIN:
        failures.append(f"uncertain {unc} != {EXPECTED_UNCERTAIN}")
    if tp + fp + unc != len(findings):
        failures.append("label sum != finding count")
    # Cross-check against the independently validated aggregate.
    agg = aggregate["classification"]
    if agg["true_positive"] != tp or agg["false_positive"] != fp \
            or agg["uncertain"] != unc:
        failures.append(
            f"transcribed labels TP/FP/UNC={tp}/{fp}/{unc} disagree with "
            f"aggregate {agg}")
    return failures


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as fh:
        for chunk in iter(lambda: fh.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--audit-dir", type=Path, default=DEFAULT_AUDIT_DIR,
                    help="path to the ry-audits/posit-corpus checkout")
    ap.add_argument("--output", type=Path, default=DEFAULT_OUTPUT,
                    help="corpus JSON to write")
    ap.add_argument("--check", action="store_true",
                    help="only validate an existing corpus against the audit")
    args = ap.parse_args(argv)

    packages_meta, aggregate, audit_results, slugs = load_audit(args.audit_dir)

    findings, problems = build_findings(audit_results, slugs)
    if problems:
        sys.exit("transcribe: audit join problems (no silent skips):\n  - " +
                 "\n  - ".join(problems))

    failures = assert_invariants(slugs, findings, aggregate)
    if failures:
        sys.exit("transcribe: invariant violations:\n  - " +
                 "\n  - ".join(failures))

    # Stable ordering: by package (audit order), then path, line, column, code.
    findings.sort(key=lambda f: (
        f["package"], f["path"], f["line"], f["column"], f["code"]))

    packages = []
    diag_by_pkg = Counter(f["package"] for f in findings)
    for slug in slugs:
        meta = packages_meta[slug]
        commit = (audit_results / slug / "git_commit").read_text().strip()
        packages.append({
            "name": slug,
            "repo": meta["repo"],
            "stars": meta["stars"],
            "commit": commit,
            "diagnostics": diag_by_pkg.get(slug, 0),
        })

    aggregate_path = args.audit_dir / "aggregate.json"
    corpus = {
        "schema_version": 1,
        "corpus": "posit",
        "ry_version": RY_VERSION,
        "source": "posit-corpus/aggregate.json",
        "source_sha256": sha256_of(aggregate_path),
        "ry_commit": RY_COMMIT,
        # The Posit ledger is an audit transcript of an installed-library run.
        # Unlike the tidyverse ledger it is not a hermetic CI baseline, so the
        # ecosystem reconciliation reports the hermetic delta rather than
        # gating on it (see ecosystem/run.sh).
        "reconciliation": "audit-transcript",
        "packages": packages,
        "findings": findings,
        "classification": {
            "true_positive": EXPECTED_TP,
            "false_positive": EXPECTED_FP,
            "uncertain": EXPECTED_UNCERTAIN,
        },
        "workstream_counts": dict(sorted(
            Counter(f["workstream"] for f in findings).items())),
        "notes": [
            "Each finding identity is transcribed from the audited ry-stdout.json "
            "and joined to the auditor's per-diagnostic classification in "
            "summary.json; the transcription script re-derives this file and "
            "asserts 34 TP / 1108 FP / 0 uncertain over 1,142 diagnostics.",
            "Workstreams are assigned deterministically from the rule code and "
            "the auditor rationale (see ecosystem/transcribe-posit-corpus.py); "
            "true positives are real defects in upstream package source.",
            "The source audit ran ry 0.8.0 with installed R libraries; ecosystem "
            "reports are hermetic (RY_NO_INSTALLED_LIBRARIES=1), so the hermetic "
            "run is compared against this transcript informationally rather than "
            "as a strict CI gate.",
        ],
    }

    if args.check:
        existing = json.loads(args.output.read_text())
        if existing != corpus:
            sys.exit(f"transcribe: {args.output} is out of date; regenerate it")
        print(f"transcribe: {args.output} matches the audit "
              f"({len(findings)} findings, {len(packages)} packages)")
        return

    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(corpus, indent=2) + "\n")
    ws = corpus["workstream_counts"]
    print(f"transcribe: wrote {args.output}")
    print(f"  packages: {len(packages)}  findings: {len(findings)}")
    print(f"  TP={EXPECTED_TP} FP={EXPECTED_FP} UNC={EXPECTED_UNCERTAIN}")
    print("  workstreams:")
    for name, count in sorted(ws.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"    {count:5d}  {name}")


if __name__ == "__main__":
    main()
