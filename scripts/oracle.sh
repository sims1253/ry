#!/usr/bin/env bash
# Run the R oracle suite (PLAN Phase 7 item 2).
#
# The oracle tests are #[ignore]'d by default so the standard CI test
# job does not require R to be installed. This script runs them when
# Rscript is available, cross-checking ry's diagnostics against R's
# actual runtime behavior.
#
# Usage:
#   scripts/oracle.sh            # run, exit nonzero on failure
#   scripts/oracle.sh --nocapture  # show per-fixture output
#
# Exits 0 if Rscript is missing (the suite is informational, not a CI
# gate): print a notice and return success so callers can run this
# unconditionally.

set -euo pipefail

if ! command -v Rscript >/dev/null 2>&1; then
  echo "oracle: Rscript not on PATH; skipping (install R to run the oracle suite)."
  exit 0
fi

cd "$(dirname "$0")/.."

EXTRA=()
if [[ "${1:-}" == "--nocapture" ]]; then
  EXTRA=(--nocapture)
fi

cargo test -p ry-checker --test oracle -- --ignored "${EXTRA[@]}"
