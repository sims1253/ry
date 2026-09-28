#!/usr/bin/env bash
# Exercise immutable-pin cache reuse through the production ecosystem harness.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_sh="$root/ecosystem/run.sh"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/ry-cached-fetch.XXXXXX")"
nonce="$(basename "$work_dir" | tr -cd 'A-Za-z0-9')"
name="fetch-$nonce"
namespace="fetch-$nonce"
package_dir="$root/ecosystem/.cache/$name"
report_stem="$namespace.$name"
reports_dir="$root/ecosystem/reports"
manifest="$work_dir/packages.txt"
ledger="$work_dir/ledger.json"
remote="$work_dir/origin.git"
url="file://$remote"
source="$work_dir/source"

cleanup() {
  if [[ -n "${RY_FETCH_TEST_KEEP:-}" ]]; then
    echo "cached-fetch fixture retained at $work_dir" >&2
    return
  fi
  rm -rf "$work_dir" "$package_dir"
  rm -f "$reports_dir/$report_stem.txt" "$reports_dir/$report_stem.full.txt"
  rm -f "$reports_dir/$report_stem.root.txt" "$reports_dir/$report_stem.root.full.txt"
  rm -f "$reports_dir/SUMMARY.$namespace.md" "$reports_dir/SUMMARY.$namespace.root.md"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

write_manifest() {
  printf '# namespace: %s\n%s\t%s\t%s\n' "$namespace" "$name" "$url" "$1" > "$manifest"
}

write_ledger() {
  cat > "$ledger" <<EOF
{
  "schema_version": 1,
  "corpus": "cached-fetch-fixture",
  "reconciliation": "hermetic",
  "packages": [{"name": "$name", "commit": "$1", "diagnostics": 2}],
  "findings": [
    {"package": "$name", "code": "RY010", "path": "R/example.R", "line": 1, "column": 1, "label": "true_positive"},
    {"package": "$name", "code": "RY010", "path": "R/example.R", "line": 2, "column": 1, "label": "false_positive"}
  ]
}
EOF
}

run_harness() {
  "$run_sh" "$@" --manifest "$manifest" --ledger "$ledger"
}

git init --quiet --bare "$remote"
git -C "$remote" config uploadpack.allowFilter true
git -C "$remote" symbolic-ref HEAD refs/heads/main
git init --quiet -b main "$source"
git -C "$source" config user.name "ry ecosystem fixture"
git -C "$source" config user.email "fixture@example.invalid"
mkdir -p "$source/R"
printf 'missing_symbol\nother_symbol\n' > "$source/R/example.R"
printf 'ignored.tmp\n' > "$source/.gitignore"
printf 'first\n' > "$source/README"
git -C "$source" add .
git -C "$source" commit --quiet -m 'fixture: first pinned source'
first="$(git -C "$source" rev-parse HEAD)"
printf 'second\n' > "$source/README"
git -C "$source" add README
git -C "$source" commit --quiet -m 'fixture: alternate source'
second="$(git -C "$source" rev-parse HEAD)"
git -C "$source" remote add origin "$url"
git -C "$source" push --quiet -u origin main

# The fixture has real R errors and the real checker must report their
# identities before the ledger is allowed to pass.
if Rscript --vanilla "$source/R/example.R" >"$work_dir/r.log" 2>&1; then
  fail "R unexpectedly accepted the fixture's unbound names"
fi
write_manifest "$first"
write_ledger "$first"
run_harness >"$work_dir/uncached.log" 2>&1
grep -F "ecosystem: cloning $name" "$work_dir/uncached.log" >/dev/null ||
  fail "the uncached run did not clone"
[[ "$(git -C "$package_dir" rev-parse HEAD)" == "$first" ]] ||
  fail "the uncached run did not check out the exact pin"
[[ "$(wc -l < "$reports_dir/$report_stem.root.txt")" -eq 2 ]] ||
  fail "the production checker did not emit both reconciled diagnostics"
run_harness --check >"$work_dir/warm.log" 2>&1
grep -F 'ecosystem: committed reports are current' "$work_dir/warm.log" >/dev/null ||
  fail "the warm run did not match committed reports"

# A stale FETCH_HEAD and dirty cached tree must not change the selected pin.
git -C "$package_dir" fetch --quiet origin "$second"
[[ "$(git -C "$package_dir" rev-parse FETCH_HEAD)" == "$second" ]] ||
  fail "the stale FETCH_HEAD control was not established"
git -C "$package_dir" checkout --quiet --detach --force "$second"
printf 'dirty\n' >> "$package_dir/R/example.R"
printf 'untracked\n' > "$package_dir/scratch.R"
printf 'ignored\n' > "$package_dir/ignored.tmp"
git -C "$package_dir" remote set-url origin "$work_dir/unavailable.git"
run_harness --check >"$work_dir/offline.log" 2>&1
[[ "$(git -C "$package_dir" rev-parse HEAD)" == "$first" ]] ||
  fail "the offline run used another revision"
[[ -z "$(git -C "$package_dir" status --porcelain --untracked-files=all)" ]] ||
  fail "the offline run left tracked or untracked changes"
[[ ! -e "$package_dir/ignored.tmp" && ! -e "$package_dir/scratch.R" ]] ||
  fail "the offline run left ignored or untracked files"
[[ "$(git -C "$package_dir" rev-parse FETCH_HEAD)" == "$second" ]] ||
  fail "the offline run unexpectedly rewrote FETCH_HEAD"

# A missing immutable object cannot fall back to cached HEAD or FETCH_HEAD.
write_manifest "0000000000000000000000000000000000000000"
if run_harness --check >"$work_dir/missing.log" 2>&1; then
  fail "an unavailable missing commit passed"
fi
[[ "$(git -C "$package_dir" rev-parse HEAD)" == "$first" ]] ||
  fail "a missing pin changed the checkout"
write_manifest "$first"

# A commit added upstream after the clone must be fetched when absent locally.
printf 'third\n' > "$source/README"
git -C "$source" add README
git -C "$source" commit --quiet -m 'fixture: new upstream source'
third="$(git -C "$source" rev-parse HEAD)"
git -C "$source" push --quiet origin main
if GIT_NO_LAZY_FETCH=1 git -C "$package_dir" cat-file -e "$third" 2>/dev/null; then
  fail "the new upstream commit was already in the cache"
fi
git -C "$package_dir" remote set-url origin "$url"
write_manifest "$third"
write_ledger "$third"
run_harness --check >"$work_dir/new-pin.log" 2>&1
grep -F "ecosystem: fetching $name at $third" "$work_dir/new-pin.log" >/dev/null ||
  fail "the absent new pin did not take the fetch path"
[[ "$(git -C "$package_dir" rev-parse HEAD)" == "$third" ]] ||
  fail "the fetched pin was not checked out"

# A partial clone can have the commit but not its promised blob. The probe
# must not fetch that blob, and checkout still needs an available origin.
rm -rf "$package_dir"
git clone --quiet --filter=blob:none --no-checkout "$url" "$package_dir"
blob="$(git -C "$source" rev-parse "$third:R/example.R")"
[[ "$(GIT_NO_LAZY_FETCH=1 git -C "$package_dir" cat-file -t "$third")" == commit ]] ||
  fail "the partial clone lacks the pinned commit object"
if GIT_NO_LAZY_FETCH=1 git -C "$package_dir" cat-file -e "$blob" 2>/dev/null; then
  fail "the partial-clone fixture unexpectedly has the promised R blob"
fi
git -C "$package_dir" remote set-url origin "$work_dir/unavailable.git"
if run_harness --check >"$work_dir/partial-offline.log" 2>&1; then
  fail "checkout succeeded offline without its promised blob"
fi
git -C "$package_dir" remote set-url origin "$url"
run_harness --check >"$work_dir/partial-online.log" 2>&1
[[ "$(git -C "$package_dir" rev-parse HEAD)" == "$third" ]] ||
  fail "the partial clone did not check out its pin with origin available"

# The cache change must preserve snapshot and strict labelled-ledger gates.
printf 'invented snapshot drift\n' >> "$reports_dir/$report_stem.root.txt"
if run_harness --check >"$work_dir/snapshot-drift.log" 2>&1; then
  fail "snapshot drift passed"
fi
grep -F "ecosystem: report drift for $report_stem.root" "$work_dir/snapshot-drift.log" >/dev/null ||
  fail "snapshot drift was not attributed to the report"
sed -i '$d' "$reports_dir/$report_stem.root.txt"
Rscript - "$ledger" "$work_dir/bad-ledger.json" <<'RS'
args <- commandArgs(trailingOnly = TRUE)
corpus <- jsonlite::fromJSON(args[[1]], simplifyVector = FALSE)
corpus$findings <- corpus$findings[-2L]
fabricated <- corpus$findings[[1L]]
fabricated$line <- 99L
corpus$findings <- c(corpus$findings, list(fabricated))
writeLines(jsonlite::toJSON(corpus, auto_unbox = TRUE, pretty = TRUE), args[[2]])
RS
if "$run_sh" --check --manifest "$manifest" --ledger "$work_dir/bad-ledger.json" \
  >"$work_dir/ledger-drift.log" 2>&1; then
  fail "missing true-positive and unowned false-positive identities passed"
fi
grep -F 'required reviewed findings disappeared' "$work_dir/ledger-drift.log" >/dev/null ||
  fail "the missing true-positive control did not fire"
grep -F 'unowned hermetic findings appeared' "$work_dir/ledger-drift.log" >/dev/null ||
  fail "the unowned false-positive control did not fire"
run_harness --check >"$work_dir/final.log" 2>&1

echo "PASS: exact cached pins, dirty-tree repair, missing/new commits, partial clone, and drift gates"
