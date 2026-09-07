#!/bin/sh
set -eu

# The pinned version keeps generated parser changes reproducible.
parser_generator=${TREE_SITTER:-tree-sitter}
case "$("$parser_generator" --version)" in
  'tree-sitter 0.24.7 '*) ;;
  *) echo 'Expected tree-sitter CLI 0.24.7' >&2; exit 1 ;;
esac
repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root/vendor/tree-sitter-r"
"$parser_generator" generate
