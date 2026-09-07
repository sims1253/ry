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

# Validate a staged copy before replacing the last usable snapshot.
staged=$(mktemp -d "${vendor}.XXXXXX")
backup=
installed=false
cleanup() {
  status=$?
  trap - EXIT
  # Finish recovery if another handled signal arrives during cleanup.
  trap '' HUP INT TERM
  if [[ -n "$backup" ]]; then
    if [[ -e "$backup/snapshot" || -L "$backup/snapshot" ]]; then
      if [[ "$installed" == true ]]; then
        rm -rf "$backup"
      elif { [[ ! -e "$vendor" && ! -L "$vendor" ]] || rm -rf "$vendor"; } \
        && mv "$backup/snapshot" "$vendor"; then
        rmdir "$backup"
        echo "ry: restored the previous typeshed snapshot after installation failed or was interrupted" >&2
      else
        echo "ry: could not restore the previous typeshed snapshot; recover it from $backup/snapshot" >&2
        [[ "$status" != 0 ]] || status=1
      fi
    else
      rmdir "$backup"
    fi
  fi
  rm -rf "$staged"
  exit "$status"
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
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

# Both directories stay on the same filesystem. Keep the old snapshot until
# the staged directory is installed; EXIT restores it on a failed move or a
# handled signal. SIGKILL and power loss can still require manual recovery.
if [[ -e "$vendor" || -L "$vendor" ]]; then
  backup=$(mktemp -d "${vendor}.backup.XXXXXX")
  mv "$vendor" "$backup/snapshot"
fi
mv "$staged" "$vendor"
installed=true
