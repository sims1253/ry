#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C.UTF-8

if [[ $# -ne 2 || ! -x "$1" || ! -x "$2" ]]; then
  echo "usage: $0 BASE_RY CANDIDATE_RY" >&2
  exit 2
fi

base="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
candidate="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
root="$(cd "$(dirname "$0")" && pwd)"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT
cd "$root"

run_case() {
  local stem="$1" label="$2" status=0
  shift 2
  "$@" >"$temporary/$stem.$label.stdout" 2>"$temporary/$stem.$label.stderr" || status=$?
  printf '%s\n' "$status" >"$temporary/$stem.$label.exit"
  for stream in stdout stderr exit; do
    if ! cmp -s "results/$stem.$label.$stream" "$temporary/$stem.$label.$stream"; then
      echo "mismatch: $stem $label $stream" >&2
      diff -u "results/$stem.$label.$stream" "$temporary/$stem.$label.$stream" >&2 || true
      exit 1
    fi
  done
}

for stem in masked-and predicate-mutation predicate-closure-mutation; do
  run_case "$stem" r Rscript --vanilla "$stem.R"
  run_case "$stem" base "$base" check "$stem.R" --output-format json
  run_case "$stem" candidate "$candidate" check "$stem.R" --output-format json
done

echo 'PASS: two rejected-candidate controls and the existing-gap control match all nine retained commands'
