#!/usr/bin/env bash
# Regression: default and non-default corpus snapshots must coexist. Before
# report names were manifest-scoped, the last generated corpus overwrote the
# shared package reports/SUMMARY and only that corpus could pass --check.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_sh="$root/ecosystem/run.sh"

# --local exercises the production manifest/report routing with the vendored
# glue source, so this remains fast and network-free while proving that both
# namespaces reconcile from one clean checkout.
"$run_sh" --local --check
"$run_sh" --local --check \
  --manifest "$root/ecosystem/posit-packages.txt" \
  --ledger "$root/docs/corpus/posit-0.9.0.json" \
  --tier fast

for snapshot in \
  glue.txt SUMMARY.md \
  posit.glue.txt SUMMARY.posit.md
do
  test -f "$root/ecosystem/reports/$snapshot" || {
    echo "FAIL: missing manifest-scoped snapshot: $snapshot" >&2
    exit 1
  }
done

# Same-basename custom manifests must not fall into the built-in namespace or
# each other's namespace. Exercise the content-hash fallback end to end and
# remove its temporary snapshots on every exit.
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/ry-manifest-isolation.XXXXXX")"
generated=()
cleanup() {
  rm -rf "$work_dir"
  if ((${#generated[@]})); then
    rm -f "${generated[@]}"
  fi
}
trap cleanup EXIT INT TERM

glue_entry="$(grep -m1 $'^glue\t' "$root/ecosystem/posit-packages.txt")"
prefixes=()
for variant in alpha beta; do
  manifest_dir="$work_dir/$variant"
  mkdir -p "$manifest_dir"
  manifest="$manifest_dir/packages.txt"
  printf '# distinct manifest %s\n%s\n' "$variant" "$glue_entry" > "$manifest"
  if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$manifest" | awk '{print $1}')"
  else
    digest="$(shasum -a 256 "$manifest" | awk '{print $1}')"
  fi
  prefix="manifest-${digest:0:12}"
  prefixes+=("$prefix")
  generated+=(
    "$root/ecosystem/reports/$prefix.glue.txt"
    "$root/ecosystem/reports/$prefix.glue.full.txt"
    "$root/ecosystem/reports/SUMMARY.$prefix.md"
  )
  "$run_sh" --local --manifest "$manifest"
  test -f "$root/ecosystem/reports/$prefix.glue.txt"
  test -f "$root/ecosystem/reports/SUMMARY.$prefix.md"
done

if [[ "${prefixes[0]}" == "${prefixes[1]}" ]]; then
  echo "FAIL: distinct same-basename manifests shared a report namespace" >&2
  exit 1
fi

echo "PASS: default, Posit, and same-basename custom manifests coexist"
