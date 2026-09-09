# Changelog

## [Unreleased]

## [0.9.2] - 2026-09-09

- Bundle ry 0.9.1 with fewer false positives in package code and a fix for
  cyclic serialized-data crashes.
- Reduce extension size with a minified production bundle.
- Reuse successful binary-version checks on restart. Changed binaries and
  failed checks are probed again.

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
