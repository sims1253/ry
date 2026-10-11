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

# Validate a staged copy before replacing the snapshot. vendor/ is tracked,
# so `git checkout crates/ry-typeshed/vendor` recovers an interrupted sync.
staged=$(mktemp -d "${vendor}.XXXXXX")
trap 'rm -rf "$staged"' EXIT
cp -R "$checkout/stubs/." "$staged/"
cat > "$staged/SOURCE" <<EOF
repository: https://github.com/sims1253/r-typeshed
commit: $commit
tree-state: $tree_state
stubs-sha256: $stubs_sha256
EOF

cargo run --manifest-path "$repo_root/Cargo.toml" -p ry-cli -- \
  typeshed validate "$staged"

# Validation may compile ry-typeshed against the old vendor after staging.
# Cargo checks include_str! inputs by mtime; refresh the shared embedded
# provenance file so the next build cannot reuse that older snapshot.
touch "$staged/SOURCE"

rm -rf "$vendor"
mv "$staged" "$vendor"
