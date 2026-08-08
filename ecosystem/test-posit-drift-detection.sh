#!/usr/bin/env bash
# Integration test: the real (non-`--local`) Posit lane must reject a corrupted
# reviewed identity and name that identity in its reconciliation failure.
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

# This first non-local check clones/fetches the pinned fast-tier Posit sources
# when they are not already cached, and proves the reviewed ledger is clean.
"$run_sh" --check --manifest "$manifest" --ledger "$clean" --tier fast   >"$tmp/clean.log" 2>&1

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
if "$run_sh" --check --manifest "$manifest" --ledger "$corrupt" --tier fast   >"$tmp/corrupt.log" 2>&1; then
  echo "FAIL: corrupted Posit ledger identity was not detected" >&2
  exit 1
fi
if ! grep -F -- "$expected" "$tmp/corrupt.log" >/dev/null; then
  echo "FAIL: reconciliation failed without naming corrupted identity: $expected" >&2
  tail -80 "$tmp/corrupt.log" >&2
  exit 1
fi

echo "PASS: corrupted Posit ledger identity detected and named: $expected"
