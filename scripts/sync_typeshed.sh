#!/usr/bin/env bash
set -euo pipefail

checkout=${1:-../r-typeshed}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
vendor="$repo_root/crates/ry-typeshed/vendor"
commit=$(git -C "$checkout" rev-parse --verify HEAD 2>/dev/null) || commit=UNAVAILABLE

rm -rf "$vendor"
mkdir -p "$vendor"
cp -R "$checkout/stubs/." "$vendor/"
cat > "$vendor/SOURCE" <<EOF
repository: https://github.com/sims1253/r-typeshed
commit: $commit
EOF

cargo run --manifest-path "$repo_root/Cargo.toml" -p ry-cli -- \
  typeshed validate "$vendor"
