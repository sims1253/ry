# ry

[![CI](https://github.com/sims1253/ry/actions/workflows/ci.yml/badge.svg)](https://github.com/sims1253/ry/actions/workflows/ci.yml)

`ry` is a static checker for R, written in Rust. It checks your project for
likely bugs before you run it: incompatible types, missing data frame columns,
unbound variables, invalid function calls, and more.

ry focuses on types and scope. Use it alongside `air` for formatting and
`lintr` for style checks. It uses
[tree-sitter-r](https://github.com/r-lib/tree-sitter-r) and takes inspiration
from [ty](https://github.com/astral-sh/ty).

> [!IMPORTANT]
> ry is mostly a playground for GLM and me. Large portions of the project are
> AI-generated. If you'd like to help, I'd love for you to join in.

## Install

On Linux and macOS, install the latest release with:

``` sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/sims1253/ry/releases/latest/download/ry-cli-installer.sh | sh
```

On Windows PowerShell:

``` powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/sims1253/ry/releases/latest/download/ry-cli-installer.ps1 | iex"
```

Or build from source with Rust 1.88 or newer:

``` sh
git clone https://github.com/sims1253/ry
cd ry
cargo build --release
# binary at target/release/ry
```

Per-platform archives are also attached to GitHub releases.
See [CHANGELOG.md](CHANGELOG.md) for release highlights and upgrade notes.

## Quickstart

Save this as `demo.R`:

```r
x <- "a" + 1L
d <- data.frame(mpg = c(21, 22.8), disp = c(160, 160))
mean(d$dispp)
```

Run the checker:

```sh
ry check demo.R
```

ry reports RY040 for the arithmetic on a character value and RY060 for
`dispp`, listing the available columns: `mpg` and `disp`.

To check a project, run `ry check .`. It collects `.R` and `.r` files
recursively and exits with a nonzero status when it finds an error.
Use `ry check --error-on-warning .` to fail on warnings too.

## Configure a project

Put a `ry.toml` in the project root. For example:

```toml
# Packages attached outside the checked source files.
packages = ["dplyr"]

# Names supplied by your application at runtime.
globals = ["runtime_data"]

exclude = ["renv", "tests/snaps/**"]
```

ry recognizes package imports, data masking, and common tidyverse patterns.
Package signatures come from the bundled
[r-typeshed](https://github.com/sims1253/r-typeshed) stubs. Dynamic R code and
missing package signatures can still produce false positives or missed bugs.
See [package awareness and known limits](docs/usage.md#package-awareness).

For an existing codebase, accept current findings in a baseline and check for
new ones:

```sh
ry check --write-baseline ry-baseline.json .
ry check --baseline ry-baseline.json .
```

See [configuration](docs/configuration.md) for rule settings, custom stubs,
inline suppressions, confidence filters, and baselines.

## Editors

Install **ry** from the
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=sims1253.ry)
or Positron's Open VSX gallery. The extension bundles the checker and provides
diagnostics, inferred type hints, and suppression actions.

See the [VS Code / Positron guide](editors/code/README.md) for settings and the
[editor setup instructions](docs/usage.md#editors) for Zed and other LSP clients.

## Reference and contributing

- [Usage](docs/usage.md): package handling, data masking, CI, and `dump-types`.
- [Rules](docs/rules.md): diagnostic codes and default severities.
  Run `ry explain rule RY040` for one rule or `ry explain rule` for all rules.
- [Changelog](CHANGELOG.md): release notes and upgrade guidance.
- [Contributing](CONTRIBUTING.md): build checks, fixtures, and typeshed updates.
