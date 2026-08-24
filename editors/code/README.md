# ry — Static type checker for R

[ry](https://github.com/sims1253/ry) is a static type checker for R. This extension provides diagnostics as you type (debounced, cached parses), inlay hints, and quick-fix actions that insert suppression comments in VS Code and Positron.

Diagnostics cover the whole project, exactly as `ry check` does. Inlay hints and quick-fix actions apply to the open document only — they do not search unopened files on disk.

## Features

- **Type-checking diagnostics** as you type (debounced, cached parses)
- **Inlay hints** showing inferred types
- **Code actions** to insert suppression comments
- **Configurable severity** — `ignore`, `error`, and `warn` settings

## Installation

### VS Code

Install from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=sims1253.ry).

### Positron

Install from Open VSX via Positron's extension gallery.

## Settings

| Setting                          | Default           | Description                                                     |
| :------------------------------- | :---------------- | :-------------------------------------------------------------- |
| `ry.enable`                      | `true`            | Enable/disable the language server                              |
| `ry.path`                        | `[]`              | Ordered list of candidate `ry` executables; first existing wins |
| `ry.importStrategy`              | `fromEnvironment` | `fromEnvironment` or `useBundled`                               |
| `ry.configuration`               |                   | Path to a `ry.toml`, overriding discovery                       |
| `ry.lint.select`                 | `[]`              | Rules to select (replaces defaults)                             |
| `ry.lint.extendSelect`           | `[]`              | Additional rules to enable                                      |
| `ry.lint.ignore`                 | `[]`              | Rules to suppress                                               |
| `ry.lint.error`                  | `[]`              | Rules to treat as errors                                        |
| `ry.lint.warn`                   | `[]`              | Rules to treat as warnings                                      |
| `ry.minConfidence`               | `low`             | Minimum confidence (`low`, `medium`, `high`)                    |
| `ry.baseline`                    |                   | Path to a baseline diagnostics file                             |
| `ry.checkTestFixtures`           | `false`           | Check fixture data under `tests/`                               |
| `ry.logLevel`                    | `warn`            | Server log level                                                |
| `ry.addExecutableToTerminalPath` | `true`            | Add `ry` to terminal `PATH`                                     |

## Commands

| Command               | Description                                       |
| :-------------------- | :------------------------------------------------ |
| `ry.restart`          | Restart the language server                       |
| `ry.showLogs`         | Show the extension log                            |
| `ry.showServerLogs`   | Show the server's stderr log                      |
| `ry.debugInformation` | Dump binary path, version, strategy, and settings |
| `ry.explainRule`      | Show the explanation for a rule                   |

## Known limitations

**Diagnostics cover only files you have open in the editor.** `ry check .`
may report additional findings in files you haven't opened. This is a
known limitation being addressed in incremental core work.

## License

MIT
