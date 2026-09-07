# Changelog

## [Unreleased]

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
