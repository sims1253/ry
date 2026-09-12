# Changelog

## [Unreleased]

## [0.10.0] - 2026-09-12

- Bundle ry 0.10.0 with improved package imports, enclosing-function lookup,
  named data-mask arguments, and condition diagnostics.
- Add packages for 32-bit ARM Linux and Alpine Linux on x64 and ARM64.
- Let `include-build-ignored` in `ry.toml` add selected build-ignored files
  to editor indexing. Existing file and resource limits still apply.
- Bound serialized-data parser nesting and collection storage; limited
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
