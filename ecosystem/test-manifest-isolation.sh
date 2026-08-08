#!/usr/bin/env bash
# Regression: default and non-default corpus snapshots must coexist. Before
# report names were manifest-scoped, the last generated corpus overwrote the
# shared package reports/SUMMARY and only that corpus could pass --check.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_sh="$root/ecosystem/run.sh"

# --local exercises the production manifest/report routing with the vendored
# glue source, so this remains fast and network-free while proving that both
# namespaces reconcile from one clean checkout.
"$run_sh" --local --check
"$run_sh" --local --check \
  --manifest "$root/ecosystem/posit-packages.txt" \
  --ledger "$root/docs/corpus/posit-0.9.0.json" \
  --tier fast

for snapshot in \
  glue.txt SUMMARY.md \
  posit.glue.txt SUMMARY.posit.md
do
  test -f "$root/ecosystem/reports/$snapshot" || {
    echo "FAIL: missing manifest-scoped snapshot: $snapshot" >&2
    exit 1
  }
done

echo "PASS: default and Posit manifest snapshots coexist"
