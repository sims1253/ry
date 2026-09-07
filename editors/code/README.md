# ry: static checker for R

[ry](https://github.com/sims1253/ry) checks R code as you type. The extension
provides project diagnostics, inlay hints for inferred types, and code actions
to insert suppression comments. Hints and code actions apply to open documents.
Type hints describe inferred assignment values, including locals in named
functions. Unknown types and assignments the checker does not visit are omitted.

## Installation

Install from the
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=scholzmx.ry-checker)
or Positron's Open VSX gallery, then open an R file.

The extension bundles ry. By default it uses an executable on `PATH` when
available, then falls back to the bundled copy. Set `ry.importStrategy` to
`useBundled` to use the bundled copy, or set `ry.path` to an ordered list of
executables. Explicit paths take precedence over the import strategy.
In untrusted workspaces, the extension uses the bundled binary.

Put a `ry.toml` in your project to configure the checker. See the
[configuration guide](https://github.com/sims1253/ry/blob/main/docs/configuration.md)
for package declarations, rule settings, and suppressions.

## Settings

| Setting                | Default           | Description                                                       |
| :--------------------- | :---------------- | :---------------------------------------------------------------- |
| `ry.enable`            | `true`            | Enable/disable the language server                                |
| `ry.path`              | `[]`              | Ordered list of candidate `ry` executables; first executable wins |
| `ry.importStrategy`    | `fromEnvironment` | `fromEnvironment` or `useBundled`                                 |
| `ry.configuration`     |                   | Path to a `ry.toml`, overriding discovery                         |
| `ry.lint.select`       | `[]`              | Rules to select (replaces defaults)                               |
| `ry.lint.extendSelect` | `[]`              | Additional rules to enable                                        |
| `ry.lint.ignore`       | `[]`              | Rules to suppress                                                 |
| `ry.lint.error`        | `[]`              | Rules to treat as errors                                          |
| `ry.lint.warn`         | `[]`              | Rules to treat as warnings                                        |
| `ry.minConfidence`     | `low`             | Minimum confidence (`low`, `medium`, `high`)                      |
| `ry.baseline`          |                   | Path to a baseline diagnostics file                               |
| `ry.logLevel`          | `warn`            | Server log level                                                  |

To check fixture data under `tests/`, set `check-test-fixtures = true` in
your project's `ry.toml`.

## Commands

| Command               | Description                                       |
| :-------------------- | :------------------------------------------------ |
| `ry.restart`          | Restart the language server                       |
| `ry.showLogs`         | Show the extension log                            |
| `ry.showServerLogs`   | Show the server's stderr log                      |
| `ry.debugInformation` | Dump binary path, version, strategy, and settings |
| `ry.explainRule`      | Show the explanation for a rule                   |

## License

MIT
