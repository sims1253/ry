#!/usr/bin/env bash
# Integration test: the ecosystem drift detector must catch a corrupted
# snapshot in the committed .txt report variant.
#
# Only .txt files are committed and tracked; .full.txt variants are
# gitignored and generated at runtime. This test corrupts the committed
# .txt report and asserts --check exits non-zero.
#
# Requires: a built release binary, Rscript + jsonlite (same deps as run.sh).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
reports_dir="$root/ecosystem/reports"
run_sh="$root/ecosystem/run.sh"

# Use the local (vendored glue) path: no git cloning, fast, hermetic. This
# intentionally tests report drift only: --local skips ledger reconciliation;
# test-posit-drift-detection.sh covers the real non-local ledger path.

# Pin the #50 invariant where it runs locally too: gitignored .full.txt
# reports are never drift baselines. Plant a poisoned sentinel before the
# first --check; current logic ignores it, so --check must still exit 0,
# while a drift-check change that starts comparing .full.txt fails here on
# any machine instead of surfacing only as a CI failure.
sentinel="$reports_dir/glue.full.txt"
printf 'POISONED_FULL_TXT_BASELINE\n' > "$sentinel"
trap 'rm -f "$sentinel"' EXIT
trap 'rm -f "$sentinel"; exit 130' INT TERM

"$run_sh" --local --check 2>/dev/null
echo "PASS: gitignored .full.txt report is not used as a baseline"

rm -f "$sentinel"
trap - EXIT INT TERM

fail=0
tested=0

# Only test .txt variants — .full.txt files are gitignored and never
# committed, so there is no committed baseline to drift against.
for variant in "glue.txt"; do
    path="$reports_dir/$variant"
    if [[ ! -f "$path" ]]; then
        echo "FAIL: expected report variant $variant does not exist"
        fail=1
        continue
    fi

    cp "$path" "$path.bak"
    trap 'cp "$path.bak" "$path" 2>/dev/null; rm -f "$path.bak"' EXIT
    trap 'cp "$path.bak" "$path" 2>/dev/null; rm -f "$path.bak"; exit 130' INT TERM

    echo "DRIFT_DETECTED" >> "$path"

    tested=$((tested + 1))
    if "$run_sh" --local --check 2>/dev/null; then
        echo "FAIL: corrupted $variant was not detected"
        fail=1
    else
        echo "PASS: corrupted $variant detected"
    fi

    cp "$path.bak" "$path"
    rm -f "$path.bak"
    trap - EXIT INT TERM
done

if [[ $tested -eq 0 ]]; then
    echo "FAIL: no report variants were tested"
    fail=1
fi

exit $fail
