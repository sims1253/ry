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

for variant in "glue.txt" "glue.full.txt"; do
    path="$reports_dir/$variant"
    [[ -f "$path" ]] || { echo "skip: $variant does not exist"; continue; }

    cp "$path" "$path.bak"
    trap 'cp "$path.bak" "$path" 2>/dev/null; rm -f "$path.bak"' EXIT

    echo "DRIFT_DETECTED" >> "$path"

    if "$run_sh" --local --check 2>/dev/null; then
        echo "FAIL: corrupted $variant was not detected"
        fail=1
    else
        echo "PASS: corrupted $variant detected"
    fi

    cp "$path.bak" "$path"
    rm -f "$path.bak"
    trap - EXIT
done

exit $fail
