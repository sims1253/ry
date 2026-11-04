#!/usr/bin/env bash
# Start one cargo-dist release binary before upload: `ry --version` must
# name the workspace version, and `ry check` must report RY040 with exit
# code 1 on the release runbook's manual smoke fixture.
# Usage: smoke-binary.sh <target> <artifacts-dir>
set -euo pipefail

target=$1
artifacts=$2

case "$target" in
  # dist cross-compiles these on a runner of another architecture (x64
  # windows-2022 and ubuntu-22.04), which cannot start them.
  aarch64-pc-windows-msvc | armv7-unknown-linux-gnueabihf)
    echo "Skipping $target: cross-compiled, cannot run on this runner"
    exit 0
    ;;
esac

work=$(mktemp -d)
if [[ $target == *-windows-* ]]; then
  unzip -q "$artifacts/ry-cli-$target.zip" -d "$work"
  ry=$work/ry.exe
else
  tar -xzf "$artifacts/ry-cli-$target.tar.gz" -C "$work"
  ry=$work/ry-cli-$target/ry
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
actual=$("$ry" --version)
echo "$actual"
if [[ $actual != "ry $version" ]]; then
  echo "::error::$target: expected 'ry $version' from --version, got '$actual'"
  exit 1
fi

# A fresh folder, so no ry.toml or DESCRIPTION above it changes the result.
mkdir "$work/smoke"
printf 'x <- "hello"\nx + 1L\n' > "$work/smoke/smoke.R"
status=0
output=$(cd "$work/smoke" && "$ry" check smoke.R 2>&1) || status=$?
echo "$output"
if [[ $status != 1 || $output != *"smoke.R:2:1: error: [RY040]"* ]]; then
  echo "::error::$target: expected RY040 at smoke.R:2:1 and exit code 1, got exit code $status"
  exit 1
fi
echo "$target: smoke check passed"
