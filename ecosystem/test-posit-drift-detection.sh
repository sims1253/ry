#!/usr/bin/env bash
# Integration test: the real (non-`--local`) Posit lane must reject a corrupted
# reviewed identity and name that identity in its reconciliation failure.
#
# Usage: ecosystem/test-posit-drift-detection.sh [REPORTS_DIR]
# REPORTS_DIR is the --reports-out directory of a passing Posit `run.sh --check`.
# Without it, the test runs that check on the fast tier first.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_sh="$root/ecosystem/run.sh"
manifest="$root/ecosystem/posit-packages.txt"
ledger="$root/docs/corpus/posit-0.9.0.json"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/ry-posit-drift.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

clean="$tmp/clean.json"
corrupt="$tmp/corrupt.json"
identity_file="$tmp/corrupt-identity.txt"
cp "$ledger" "$clean"

# run.sh exports reports only after they match the snapshots and the clean
# ledger reconciles.
reports="${1:-}"
if [[ -z "$reports" ]]; then
  reports="$tmp/reports"
  if ! "$run_sh" --check --manifest "$manifest" --ledger "$clean" --tier fast \
    --reports-out "$reports" >"$tmp/clean.log" 2>&1; then
    cat "$tmp/clean.log" >&2
    exit 1
  fi
fi

Rscript - "$clean" "$corrupt" "$identity_file" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
ledger <- jsonlite::fromJSON(args[[1]], simplifyVector = FALSE)
idx <- which(vapply(ledger$findings, function(x) identical(x$package, "glue"), logical(1)))[[1]]
finding <- ledger$findings[[idx]]
finding$column <- finding$column + 100000L
ledger$findings[[idx]] <- finding
writeLines(jsonlite::toJSON(ledger, auto_unbox = TRUE, pretty = TRUE), args[[2]])
writeLines(sprintf(
  "%s\t%s\t%s\t%s\t%s",
  finding$package, finding$code, finding$path, finding$line, finding$column
), args[[3]])
RS

expected="$(cat "$identity_file")"
# Only the ledger changed. Reconcile against fresh current-binary reports
# from a successful non-local run, not committed or cached reports.
processed=()
while IFS= read -r package; do processed+=("$package"); done < "$reports/packages.txt"
report_prefix="$(cat "$reports/report-prefix.txt")"
if Rscript "$root/ecosystem/reconcile-reports.R" "$corrupt" "$reports" \
  "$report_prefix" "${processed[@]}" >"$tmp/corrupt.log" 2>&1; then
  echo "FAIL: corrupted Posit ledger identity was not detected" >&2
  exit 1
fi
if ! grep -F -- "$expected" "$tmp/corrupt.log" >/dev/null; then
  echo "FAIL: reconciliation failed without naming corrupted identity: $expected" >&2
  tail -80 "$tmp/corrupt.log" >&2
  exit 1
fi

echo "PASS: corrupted Posit ledger identity detected and named: $expected"
