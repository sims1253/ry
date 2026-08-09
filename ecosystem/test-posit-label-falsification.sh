#!/usr/bin/env bash
# P35-W11: Corpus reconciliation falsification.
#
# Prove the ecosystem check actually fails when a true_positive finding
# disappears and a false_positive is added without updating the ledger.
# The existing test-posit-drift-detection.sh corrupts a finding's column
# (position drift). This test corrupts the FINDING LABELS: it adds a
# true_positive identity that does not exist in the reports (simulating a
# real bug that the checker stopped catching) and removes a false_positive
# identity (so the report's entry becomes unowned, simulating a new false
# positive appearing without ledger coverage).
#
# Both corruptions are in the same run so the test proves the reconciliation
# gate catches simultaneous TP-disappearance and FP-appearance — the exact
# silent-degradation scenario W11 names.
#
# Requires: a built release binary, Rscript + jsonlite, and network access
# to clone the fast-tier Posit sources (same deps as test-posit-drift-detection.sh).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_sh="$root/ecosystem/run.sh"
manifest="$root/ecosystem/posit-packages.txt"
ledger="$root/docs/corpus/posit-0.9.0.json"
tmp="$(mktemp -d "${TMPDIR:-/tmp}/ry-label-falsify.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

clean="$tmp/clean.json"
corrupt="$tmp/corrupt.json"
cp "$ledger" "$clean"

# First: prove the reviewed ledger is clean (this also warms the cache).
"$run_sh" --check --manifest "$manifest" --ledger "$clean" --tier fast \
  >"$tmp/clean.log" 2>&1

# Corrupt the ledger: remove one false_positive and add one true_positive
# at a fabricated identity that will never appear in the reports.
Rscript - "$clean" "$corrupt" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
ledger <- jsonlite::fromJSON(args[[1]], simplifyVector = FALSE)

findings <- ledger$findings

# Remove the first false_positive: the report still emits it, so it
# becomes an unowned finding (actual but not in the ledger).
fp_idx <- which(vapply(findings, function(x) identical(x$label, "false_positive"), logical(1)))[[1]]
removed_fp <- findings[[fp_idx]]
findings <- findings[-fp_idx]

# Add a true_positive at a fabricated identity: the reports will never
# contain it, so it becomes a missing_required finding (true_positive
# in the ledger but absent from actual reports).
fabricated <- list(
  package = removed_fp$package,
  code = "RY010",
  path = "R/__falsified_disappeared_tp__.R",
  line = 9999L,
  column = 9999L,
  label = "true_positive",
  workstream = "falsification"
)
findings <- c(findings, list(fabricated))

ledger$findings <- findings
writeLines(jsonlite::toJSON(ledger, auto_unbox = TRUE, pretty = TRUE), args[[2]])

message("removed false_positive: ", removed_fp$package, "\t", removed_fp$code, "\t", removed_fp$path, ":", removed_fp$line, ":", removed_fp$column)
message("added true_positive: ", fabricated$package, "\t", fabricated$code, "\t", fabricated$path, ":", fabricated$line, ":", fabricated$column)
RS

# The corrupted ledger must fail reconciliation.
if "$run_sh" --check --manifest "$manifest" --ledger "$corrupt" --tier fast \
  >"$tmp/corrupt.log" 2>&1; then
  echo "FAIL: corrupted ledger (removed FP + added missing TP) was not detected" >&2
  exit 1
fi

# Verify the failure names BOTH a missing_required (the fabricated TP)
# and an unowned finding (the removed FP).
missing_required_ok=false
unowned_ok=false

# The fabricated true_positive should appear as a disappeared required finding.
if grep -F "__falsified_disappeared_tp__.R" "$tmp/corrupt.log" >/dev/null 2>&1; then
  missing_required_ok=true
fi

# The removed false_positive should appear as an unowned finding.
# Its identity in the log is package-encoded; check for its path.
removed_fp_path="$(Rscript -e '
args <- commandArgs(trailingOnly = TRUE)
ledger <- jsonlite::fromJSON(args[[1]], simplifyVector = FALSE)
fp_idx <- which(vapply(ledger$findings, function(x) identical(x$label, "false_positive"), logical(1)))[[1]]
fp <- ledger$findings[[fp_idx]]
cat(fp$path)
' "$clean")"
if grep -F "$removed_fp_path" "$tmp/corrupt.log" >/dev/null 2>&1; then
  unowned_ok=true
fi

if ! $missing_required_ok; then
  echo "FAIL: reconciliation did not name the disappeared true_positive" >&2
  cat "$tmp/corrupt.log" >&2
  exit 1
fi
if ! $unowned_ok; then
  echo "FAIL: reconciliation did not name the unowned false_positive" >&2
  cat "$tmp/corrupt.log" >&2
  exit 1
fi

echo "PASS: simultaneous TP disappearance and FP appearance detected by ledger reconciliation"
