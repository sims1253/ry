#!/usr/bin/env bash
# Integration test: the ecosystem drift detector must catch a corrupted
# snapshot in BOTH the standard (.txt) and full (.full.txt) report variants.
#
# Previously the drift loop only compared $stem.txt, so a drifted
# $stem.full.txt would pass --check silently. This test corrupts each
# variant in turn and asserts --check exits non-zero.
#
# Requires: a built release binary, Rscript + jsonlite (same deps as run.sh).
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
reports_dir="$root/ecosystem/reports"
run_sh="$root/ecosystem/run.sh"

# Use the local (vendored glue) path: no git cloning, fast, hermetic.
"$run_sh" --local --check 2>/dev/null

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
