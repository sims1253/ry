#!/usr/bin/env bash
set -euo pipefail

checkout=${1:-../r-typeshed}
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
vendor="$repo_root/crates/ry-typeshed/vendor"
commit=$(git -C "$checkout" rev-parse --verify HEAD 2>/dev/null) || commit=UNAVAILABLE
tree_state=clean
if [[ -n $(git -C "$checkout" status --porcelain 2>/dev/null) ]]; then
  tree_state=dirty
fi
stubs_sha256=$(
  cd "$checkout/stubs"
  find . -type f -print0 \
    | LC_ALL=C sort -z \
    | xargs -0 sha256sum \
    | sha256sum \
    | cut -d' ' -f1
)

rm -rf "$vendor"
mkdir -p "$vendor"
cp -R "$checkout/stubs/." "$vendor/"
cat > "$vendor/SOURCE" <<EOF
repository: https://github.com/sims1253/r-typeshed
commit: $commit
tree-state: $tree_state
stubs-sha256: $stubs_sha256
EOF

cargo run --manifest-path "$repo_root/Cargo.toml" -p ry-cli -- \
  typeshed validate "$vendor"
