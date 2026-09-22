# Changelog

## [Unreleased]

## [0.11.1] - 2026-09-22

- Fix the banner and logo in the Marketplace and Open VSX readme: vsce
  rewrote the relative image paths against the repository root, where
  `editors/code/` paths do not resolve, so both images showed only their
  alt text. The readme now carries absolute URLs, and the unused webp
  copies stay out of the VSIX (`icon.png` remains as the listing icon).
  Extension-only release bundling the same verified ry 0.11.0 core
  binaries (core tag `v0.11.0`).

## [0.11.0] - 2026-09-22

- Bundle ry 0.11.0 with the six new rules RY106-RY111, superassignment and
  dynamically constructed closure modeling, and the RY000 pipe-RHS,
  non-UTF-8, and byte-order-mark parser-trust checks.
- Bundle the language server's watched-file convergence work: edits,
  creations, and deletions of unopened R files refresh the index and
  republish diagnostics, stale diagnostics clear when a file leaves
  analysis, and configuration reloads converge with a fresh server.
- Defer loading the language client and Effect machinery until after
  activation completes: the benchmarked activation time drops from
  ~153 ms to ~87 ms with identical server startup behavior. The client
  log channel is now created lazily on first use instead of at module
  load.

## [0.10.0] - 2026-09-13

- Bundle ry 0.10.0 with improved package imports, enclosing-function lookup,
  named data-mask arguments, and condition diagnostics, plus the sibling
  `test_that` scope, data.table select-subscript, and data-mask column
  fixes from the post-qualification round (#368, #367, #369).
- Add an extension icon for VS Code and Positron (#456).
- Add packages for 32-bit ARM Linux and Alpine Linux on x64 and ARM64.
- Let `include-build-ignored` in `ry.toml` add selected build-ignored files
  to editor indexing. Existing file and resource limits still apply.
- Bound serialized-data parser nesting and collection storage; over-cap
  inventories appear in the language-server log.
- Refresh the binary version check after replacements that preserve file size
  and modification time (#429).

## [0.9.2] - 2026-09-10

- Bundle ry 0.9.2 with fewer false positives in package code and a fix for
  cyclic serialized-data crashes.
- Reduce extension size with a minified production bundle.
- Reuse successful version checks when the language server restarts within
  the same editor session. Changed binaries and failed checks are probed again.
- New diagnostics can appear after upgrading: multi-value `if`/`while`
  conditions now surface as RY001, and RY001/RY070 message text changed, so
  accepted-baseline entries that match the old messages can reappear. See the
  [core changelog](../../CHANGELOG.md) before regenerating baselines.

## [0.9.1] - 2026-09-07

- Publish on VS Code Marketplace as `scholzmx.ry-checker`, displayed as
  `ry - R Type Checker`. Open VSX retains `scholzmx.ry`.

## [0.9.0] - 2026-09-07

### Added

- Initial VS Code / Positron extension for ry.
- Bundled binary support with platform-specific VSIX packaging.
- Binary resolution: `ry.path`, `fromEnvironment`, `useBundled` import strategies.
- Settings surface: `ry.lint.*`, `ry.minConfidence`, `ry.baseline`, `ry.logLevel`.
- Restart orchestration with coalescing.
- Language status item showing resolved binary and version.
- Commands: `ry.restart`, `ry.showLogs`, `ry.showServerLogs`, `ry.debugInformation`, `ry.explainRule`.
