# Release runbook

This document covers the release process for the ry core binary, the
VS Code extension, and the Zed extension. Follow these steps in order.

The binary release and VS Code publication each require a manual workflow
dispatch. Publish and verify the core first, then publish the VS Code extension,
then submit the Zed gallery pull request. Each product can be released separately.
Use Python 3.11 or newer for the release checksum tests (`tomllib` is required).

## Pre-release checklist

Before starting any release:

1. **All gates green:**
   - `cargo test --workspace`
   - `cargo test -p ry-checker --test oracle --test semantic_lists -- --include-ignored`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `cargo fmt --all -- --check`
   - `cargo +nightly fuzz run parse -- -max_total_time=300 -max_len=4096`
   - `cargo test -p ry-lsp --test session_state_machine -- --ignored`
   - `ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --ledger docs/corpus/posit-0.9.0.json --tier fast`
   - `ecosystem/test-drift-detection.sh`
   - `ecosystem/test-posit-drift-detection.sh`

2. **Tracked-only build validated:** every PR's `ecosystem` CI job builds
   `--locked` from a fresh checkout, which contains only tracked files; for
   the exact commit about to be tagged, validate directly with a
   tracked-only `git archive` build:

   ```bash
   (
     set -euo pipefail
     tmp=$(mktemp -d)
     trap 'rm -rf "$tmp"' EXIT
     git archive HEAD | tar -x -C "$tmp"
     cargo build --release --locked --manifest-path "$tmp/Cargo.toml" -p ry-cli --bin ry
   )
   ```

   The subshell fails fast (`set -euo pipefail`), and the trap removes the
   extraction on any exit, so a failed validation cannot be masked by
   cleanup.

   The extraction contains exactly the tracked files of `HEAD`. Running
   `git clean -fdX` in the working tree is not an equivalent substitute:
   it removes only ignored files, so untracked non-ignored files survive
   and it can pass while a tracked-only build fails (#50).

3. **Ledger reconciled:** `python3 ecosystem/check-ledger.py docs/corpus/posit-0.9.0.json docs/corpus/tidyverse-0.7.1.json`
   reports agreement.

4. **CHANGELOG reviewed:** verify the Unreleased section is complete and
   dated.

5. **Version bumped:** core workspace `Cargo.toml` to the target version
   (e.g. `0.9.0`). Editor extension versions are independent.

6. **Zed binary integrity verified:** confirm the core release includes
   executable `ry-cli-<target>.bin.sha256` sidecars for all six targets.
   Run `cargo test --manifest-path editors/zed/Cargo.toml` and
   `python3 -m unittest discover -s scripts/release -p 'test_*.py'` to check
   download verification and sidecar generation. Archive `.sha256` files
   verify archives; executable sidecars verify extracted binaries. The
   cargo-dist checksum hook must pass before publication. See the
   [Zed release steps](#zed-extension-release) for generation and rollout.

## Binary release

### Tag format

```
v{version}  (e.g. v0.9.0)
```

### Steps

1. Set the workspace version in `Cargo.toml` and refresh `Cargo.lock` with
   `cargo check --workspace`. If both already contain the target version,
   no version change is needed.
2. Update `CHANGELOG.md`: move `[Unreleased]` to `[version] - YYYY-MM-DD`.
3. Commit the release preparation and merge it to the release branch. Run the
   pre-release gates on that commit, including a binary dry run:

   ```bash
   gh workflow run release.yml --ref <release-branch> -f tag=dry-run
   ```

   Record the run's commit SHA. Require all six platform builds and
   `custom-binary-checksums` to succeed, with `host` skipped. Inspect the
   `artifacts-binary-checksums` artifact for six `.bin.sha256` files.
4. Tag that exact reviewed commit: `git tag v{version} <reviewed-commit-sha>`.
5. Push tag: `git push origin v{version}`.
6. Dispatch the release explicitly against the tag:

   ```bash
   gh workflow run release.yml --ref v{version} -f tag=v{version}
   ```

   A tag push alone does not start this workflow. The dispatch builds and
   publishes the GitHub Release. **cargo-dist** produces:
   - Six platform binaries (x86_64/aarch64 × linux/macOS/windows)
   - SHA-256 sidecar files for each archive
   - Six executable `.bin.sha256` sidecars from the checksum hook
   - GitHub Release with all assets attached
7. Verify: download each archive and its `.sha256` sidecar, run
   `sha256sum -c archive.sha256`, extract, and run `ry version`.

### Artifact verification checklist

- [ ] Six platform archives exist in the GitHub release
- [ ] Each archive has a matching `.sha256` sidecar
- [ ] All six `ry-cli-<target>.bin.sha256` executable sidecars exist
- [ ] `sha256sum -c` passes for every archive
- [ ] `ry version` reports the correct version on each platform
- [ ] `ry check` runs successfully on a simple test file

## VS Code extension release

### Prerequisites

- Publisher identity verified: `scholzmx.ry` across `package.json`,
  `constants.ts`, `README.md`, and both test-suite `getExtension` lookups
  (enforced by the `publisher-consistency` job).
- Core binary release tag exists with verified artifacts.
- The Marketplace publisher `scholzmx` exists and the `VSCE_PAT` repository
  secret can publish under it. The Open VSX namespace `scholzmx` exists,
  its publishing agreement is signed, and `OVSX_PAT` can publish under it.
  Secret names alone do not prove that credentials are valid. See the
  [Marketplace publishing guide](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
  and [Open VSX publishing guide](https://github.com/eclipse-openvsx/openvsx/wiki/Publishing-Extensions).
- Complete the core artifact verification checklist before dispatch. The
  extension release workflow checks archive integrity and binary presence;
  it does not execute the downloaded binaries. PR extension tests use a
  locally built Linux binary, not the published core archives.

### Steps

1. Dispatch `release-vscode.yml` from the reviewed extension source ref with:
   - `version`: extension SemVer (e.g. `0.1.0`)
   - `core-tag`: the core binary tag (e.g. `v0.9.0`)
   - `pre-release`: true/false

   For a stable 0.9.0 extension built from the core release commit:

   ```bash
   gh workflow run release-vscode.yml --ref v0.9.0 \
     -f version=0.9.0 -f core-tag=v0.9.0 -F pre-release=false
   ```

   This command publishes to both registries; it is not a packaging dry run.

2. The workflow:
   - Downloads the core binary for each platform from the specified tag
   - Verifies SHA-256 checksums
   - Packages platform-specific VSIXs
   - Publishes to VS Code Marketplace and Open VSX

3. Post-publish smoke test:
   - Install the extension from the marketplace in a clean VS Code
     installation
   - Open an R file
   - Verify diagnostics fire
   - Check the status bar shows the correct version
   - Verify the bundled binary version matches the release tag

   Set `ry.importStrategy` to `useBundled` and clear any `ry.path` override
   for this test. The default `fromEnvironment` strategy may select an older
   binary on `PATH`. Repeat in Positron using its Open VSX installation.

### Rollback

Publish a higher extension patch version containing the fix or reverted code.
An extension-only repair can package the same verified core tag. If the bundled
core caused the problem, choose a verified compatible core release instead.
Re-run the extension checks before dispatching publication.

`vsce unpublish` removes the entire extension; it is not a version rollback.
The Marketplace does not allow deletion of the latest version or reuse of a
deleted version number. See the [Marketplace removal documentation](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#removing-extensions).
Use each registry's management interface if an emergency requires withdrawing
the listing, and check the scope of that action before confirming it.

If only one registry's publication fails, rerun the failed job from that workflow
run after correcting the cause. It reuses the packaged artifacts, and
`--skip-duplicate` permits retrying targets already uploaded.

## Zed extension release

### Steps

1. Set the gallery version in `editors/zed/extension.toml`. The private Rust
   crate in `editors/zed/Cargo.toml` has a separate version.
2. Verify WASM build: `cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2`.
3. Verify tests: `cargo test --manifest-path editors/zed/Cargo.toml`.
4. Confirm the server release includes `ry-cli-<target>.bin.sha256` for all six
   targets. The cargo-dist checksum hook verifies each archive before hashing
   its executable, then uploads the sidecars with the release. A failed hook
   blocks publication. Reproduce generation with
   `python3 scripts/release/binary_checksums.py --artifacts <archive-directory> --output <sidecar-directory>`.
   Before publishing, run
   `gh workflow run release.yml --ref <release-branch> -f tag=dry-run`.
   Confirm `custom-binary-checksums` succeeds and the `artifacts-binary-checksums`
   artifact contains all six `.bin.sha256` sidecars. Confirm the `host` publication
   job is skipped; `dry-run` builds artifacts without creating a release.
5. Submit to the Zed extension gallery after the server release is available.
   Automatic downloads require executable sidecars, published from 0.9.0 onward.
   The extension verifies existing downloads and rehashes cached binaries on
   restart; missing or invalid checksums cause an error and remove that download.
   Explicit settings and PATH binaries remain user-managed.
6. Verify: install in Zed, open an R file, verify diagnostics fire.

### First gallery submission

Follow the [Zed publishing guide](https://zed.dev/docs/extensions/publishing/publishing-guide).
In a fork of `zed-industries/extensions`, add `https://github.com/sims1253/ry`
as the `extensions/ry` submodule and check out the reviewed release commit.
The commit must also be reachable from a branch in the public ry repository.
Add this entry to the gallery's `extensions.toml`:

```toml
[ry]
submodule = "extensions/ry"
path = "editors/zed"
version = "0.9.0"
```

Use the version and extension ID from `editors/zed/extension.toml` if they
change. Run `pnpm sort-extensions` in the gallery checkout. Manually test that
commit as a dev extension before submitting the pull request, as required by
the [publishing prerequisites](https://zed.dev/docs/extensions/publishing/prerequisites).
Install the R language extension as well; ry supplies a language server, not
an R grammar. Add `"ry"` to `languages.R.language_servers` in Zed's settings
(see [editor setup](usage.md#zed)). Test automatic download with no configured binary and no ry on
`PATH`, then restart Zed to exercise the cached binary check. Gallery publication
follows review and merge of the pull request.

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

### Manual smoke-test recipe

Use a fresh folder with default settings and no `ry.toml`. Create `smoke.R`:

```r
x <- "hello"
x + 1L
```

Run the release binary with `ry check smoke.R --output-format json`. Expect
RY040 at line 2, column 1 and exit code 1. Open the folder in the editor and
confirm the same diagnostic. Replace `"hello"` with `1L`, save, and confirm the
diagnostic disappears in both the editor and CLI (CLI exit code 0).
Restore the error, restart the language server or editor, and confirm the
diagnostic returns. Record the editor version, OS/architecture, binary path,
and ry version alongside the result. A passing build does not replace this
runtime check on platforms that were not exercised automatically.

## Version policy

- Core and editor extension versions are **independent**.
- Core uses SemVer (e.g. `0.9.0`).
- VS Code extension uses its own SemVer (e.g. `0.1.0`).
- Zed extension uses its own SemVer (e.g. `0.1.0`).
- Each VS Code extension release records the exact core tag it packages.
- The Zed extension uses a configured binary, a binary on `PATH`, or a cached
  download. When it needs a download, it selects the latest stable core release.
- The CHANGELOG records core version changes; extension releases appear
  in their marketplace listings.
