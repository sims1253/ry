# Release Runbook

This document covers the release process for the ry core binary, the
VS Code extension, and the Zed extension. Follow these steps in order.

## Pre-release checklist

Before starting any release:

1. **All P37 gates green:**
   - `cargo test --workspace`
   - `cargo test -p ry-core`
   - `cargo +nightly fuzz run parse -- -max_total_time=300 -max_len=4096`
   - `cargo test -p ry-lsp --test p36_contract`
   - `cargo test -p ry-lsp --test w8_session -- --ignored`
   - `ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --ledger docs/corpus/posit-0.9.0.json --tier fast`
   - `ecosystem/test-drift-detection.sh`
   - `ecosystem/test-posit-drift-detection.sh`

2. **Tracked-only build validated:** the scheduled `clean-checkout` CI job
   passes (it builds from a fresh clone, which contains only tracked
   files, but it runs only on schedule or manual dispatch and never gates
   the commit about to be tagged), or validate the release commit directly
   with a tracked-only `git archive` build:

   ```sh
   tmp=$(mktemp -d)
   git archive HEAD | tar -x -C "$tmp"
   cargo build --release --locked --manifest-path "$tmp/Cargo.toml" -p ry-cli --bin ry
   rm -rf "$tmp"
   ```

   The extraction contains exactly the tracked files of `HEAD`. Running
   `git clean -fdX` in the working tree is not an equivalent substitute:
   it removes only ignored files, so untracked non-ignored files survive
   and it can pass while a tracked-only build fails (#50).

3. **Ledger reconciled:** `python3 ecosystem/check-ledger.py docs/corpus/posit-0.9.0.json`
   reports agreement.

4. **CHANGELOG reviewed:** verify the Unreleased section is complete and
   dated.

5. **Version bumped:** core workspace `Cargo.toml` to the target version
   (e.g. `0.9.0`). Editor extension versions are independent.

## Binary release

### Tag format

```
v{version}  (e.g. v0.9.0)
```

### Steps

1. Bump version in `Cargo.toml` and `Cargo.lock` (`cargo workspaces`).
2. Update `CHANGELOG.md`: move `[Unreleased]` to `[version] - YYYY-MM-DD`.
3. Commit: `Bump version to {version}`.
4. Tag: `git tag v{version}`.
5. Push tag: `git push origin v{version}`.
6. **cargo-dist** dispatches automatically from the tag push. It produces:
   - Six platform binaries (x86_64/aarch64 × linux/macOS/windows)
   - SHA-256 sidecar files for each archive
   - GitHub Release with all assets attached
7. Verify: download each archive and its `.sha256` sidecar, run
   `sha256sum -c archive.sha256`, extract, and run `ry version`.

### Artifact verification checklist

- [ ] Six platform archives exist in the GitHub release
- [ ] Each archive has a matching `.sha256` sidecar
- [ ] `sha256sum -c` passes for every archive
- [ ] `ry version` reports the correct version on each platform
- [ ] `ry check` runs successfully on a simple test file

## VS Code extension release

### Prerequisites

- Publisher identity verified: `sims1253.ry` across `package.json`,
  `constants.ts`, and `README.md`.
- Core binary release tag exists with verified artifacts.

### Steps

1. Dispatch `release-vscode.yml` with:
   - `version`: extension SemVer (e.g. `0.1.0`)
   - `core-tag`: the core binary tag (e.g. `v0.9.0`)
   - `pre-release`: true/false

2. The workflow:
   - Downloads the core binary for each platform from the specified tag
   - Verifies SHA-256 checksums
   - Packages platform-specific VSIXs
   - Smoke-tests `ry version` and `ry check`
   - Publishes to VS Code Marketplace and Open VSX

3. Post-publish smoke test:
   - Install the extension from the marketplace in a clean VS Code
     installation
   - Open an R file
   - Verify diagnostics fire
   - Check the status bar shows the correct version
   - Verify the bundled binary version matches the release tag

### Rollback

- VS Code Marketplace: `vsce unpublish sims1253.ry@{version}`
- Open VSX: `ovsx unpublish sims1253.ry@{version}`

## Zed extension release

### Steps

1. Verify `extension.toml` and `Cargo.toml` versions are consistent.
2. Verify WASM build: `cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2`.
3. Verify tests: `cargo test --manifest-path editors/zed/Cargo.toml`.
4. Submit to the Zed extension gallery.
5. Verify: install in Zed, open an R file, verify diagnostics fire.

### Rollback

Contact Zed to unpublish the extension version.

## Post-release checks

After all artifacts are published:

1. Install from marketplace/gallery in a clean environment (not from source).
2. Verify diagnostics fire on a known-bad R file.
3. Verify the bundled binary version matches the release tag:
   - VS Code: check status bar, or run the `ry: Debug Information` command.
   - Zed: check the extension's downloaded binary version.
4. Verify CLI/LSP parity: `ry check` and the LSP produce identical diagnostics.

## Version policy

- Core and editor extension versions are **independent**.
- Core uses SemVer (e.g. `0.9.0`).
- VS Code extension uses its own SemVer (e.g. `0.1.0`).
- Zed extension uses its own SemVer (e.g. `0.1.0`).
- Each extension release records the exact core tag it packages.
- The CHANGELOG records core version changes; extension releases appear
  in their marketplace listings.
