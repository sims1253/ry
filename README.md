# ry

`ry` is a static checker for the R language. It parses R source with
[tree-sitter-r](https://github.com/r-lib/tree-sitter-r), infers types
across files, and reports likely bugs (calling a non-function, arithmetic
on incompatible types, undefined columns, unbound variables, malformed
`if` conditions, ...) before you run the code.

It is **not** a style formatter; pair it with `styler` or `air` for
formatting. It is also not a replacement for `lintr`'s stylistic rules --
ry focuses on type- and scope-driven diagnostics that need a whole-program
view.

## Status

Early. The type model covers common base-R patterns (atomic vectors,
lists, data frames, closures, S3 dispatch, higher-order functions like
`lapply`/`sapply`/`vapply`/`Map`/`Reduce`). Known gaps are tracked in
`PLAN.md`:

- No S4 / R6 / environment-as-a-hashmap modeling.
- No package `NAMESPACE` resolution (cross-package calls resolve through
  an embedded typeshed of common packages, or fall back to opaque).
- Possibly-unbound-variable diagnostics are intentionally NOT emitted
  (branch-merge logic prefers silence over a false positive -- see
  `PLAN.md` Phase A1).
- Incremental checking is per-keystroke in the LSP (no salsa); large
  workspaces may want debouncing (planned, Phase E).

The 40-fixture R oracle harness (`cargo test -p ry-checker --test oracle
-- --ignored`) is green: where ry and R disagree, the disagreement is
documented, not hidden.

## Install

ry is a Cargo workspace. Build from source:

```sh
cargo build --release
# binary at target/release/ry
```

A `rust-version` of **1.82** or newer is required (the checker uses
`Option::is_none_or` and other recent-stable APIs).

## Run

Check files or directories (recursively, `*.R` and `*.r`):

```sh
ry check             # the current directory
ry check path/to/pkg # a directory
ry check file1.R file2.R
ry check -W          # watch mode: re-check on change (500ms poll)
```

Output formats (`--output-format`): `concise` (default), `full` (concise
line plus the source line and a caret), `json`, `github`, `gitlab`,
`junit`. The human summary line is suppressed for the machine-readable
formats. `full` example:

```
pkg/foo.R:3:11: error: [RY040] cannot apply arithmetic op to `character` and `double`
    y <- "x" * 2
              ^
```

Exit codes: non-zero when any error-level diagnostic is emitted (override
with `--exit-zero`); promote warnings to errors with `--error-on-warning`.

## Configuration (`ry.toml`)

Discovered by walking up from the search start (the first path passed, or
the current directory). Keys (all optional; kebab-case aliases accepted):

```toml
# Promote / demote / disable rules by code (RY040) or name
# (invalid-arithmetic), or "all".
error = ["RY040"]
warn  = ["RY070"]
ignore = ["RY033"]

error-on-warning = false   # exit 1 on any warning
exit-zero        = false   # always exit 0
output-format    = "concise"
verbose          = 0       # additive with -v / -q on the CLI
quiet            = 0

# gitignore-style patterns, matched against the path relative to this
# ry.toml's directory.
exclude = ["tests/snaps/**", "renv"]
```

CLI scalar flags override the config ONLY when passed explicitly; a bare
`ry check` lets `ry.toml` drive severity.

## Inline suppression

Mirror the `# ruff: ignore` / `# noqa` conventions:

```r
x <- bad  # ry: ignore                 # suppress ALL rules on this line
x <- bad  # ry: ignore[RY010]          # suppress a specific rule
x <- bad  # ry: ignore[RY010, RY040]   # suppress multiple rules
x <- bad  # noqa: RY010                # flake8/ruff-compatible alias

# ry: ignore                           # standalone: suppresses the next
x <- bad                               #   non-comment, non-blank line

# ry: ignore-file                      # file-level: suppresses everything
```

## `--color`

`--color {auto|always|never}` is parsed and validated, and honors the
`NO_COLOR` environment variable. The CLI does not yet emit colorized
output (the human formats are plain text), so the choice has no observable
effect today; it exists so the flag is honest and forward-compatible.

## Language server

```sh
ry server   # speaks LSP over stdio
```

Connect from any LSP-aware editor (VS Code, Neovim, Helix, ...). Provides
diagnostics, hover (shows a variable's inferred type), go-to-definition,
references, rename, document symbols, inlay hints, completion, signature
help, folding, selection range, and code actions.

## Rule reference

Default severities can be overridden per-project in `ry.toml`. See
`ry explain rule` at the terminal for the same table. Summaries below are
verbatim from `crates/ry-checker/src/rules.rs` (the canonical source).

| Code   | Name                     | Default  | Summary                                                                                                                                                                                                               |
|--------|--------------------------|----------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| RY000  | syntax-error             | error    | Unparseable input. tree-sitter could not recover this region; subsequent diagnostics may be unreliable.                                                                                                              |
| RY001  | invalid-condition        | warning  | `if` / `while` condition is not a length-1 logical.                                                                                                                                                                   |
| RY002  | condition-length         | warning  | `if` condition has length > 1; only the first element is used.                                                                                                                                                       |
| RY010  | unbound-variable         | warning  | Reference to a variable with no binding in scope.                                                                                                                                                                     |
| RY020  | unary-minus-type         | error    | Unary `-` applied to a non-numeric type.                                                                                                                                                                              |
| RY021  | unary-not-type           | error    | Unary `!` applied to a non-coercible-to-logical type.                                                                                                                                                                 |
| RY030  | invalid-comparison       | error    | Comparison between types with no defined ordering.                                                                                                                                                                    |
| RY031  | invalid-logical-op       | error    | `&` / `\|` / `&&` / `\|\|` applied to non-coercible types.                                                                                                                                                            |
| RY032  | scalar-logical-length    | warning  | `&&` and `||` only use the first element of their operands; using them with vectors of length > 1 is almost always a bug. Use `&`/`\|` for vectorized operations.                                                    |
| RY033  | comparison-mode-mismatch| warning  | Comparing a character value with a numeric value is valid R but almost always unintended. R compares byte values, not semantic equality.                                                                              |
| RY040  | invalid-arithmetic       | error    | Arithmetic operator between incompatible types.                                                                                                                                                                       |
| RY050  | missing-s3-method        | warning  | S3 generic called on a value with no defined method for its class.                                                                                                                                                    |
| RY060  | undefined-column         | error    | Column access on a value whose schema does not contain that column.                                                                                                                                                   |
| RY061  | dollar-on-atomic         | error    | The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).                                                            |
| RY070  | call-non-function        | error    | A non-function value (a variable bound to a non-function, or a literal like `42()`) is being called as a function. R will error at runtime ('attempt to apply non-function' / 'could not find function').             |

## Oracle

The R oracle harness compares ry's diagnostics against `Rscript`'s
behavior on a corpus of fixtures. It is `#[ignore]`'d by default (it
needs R installed) and runs in CI as a separate job.

```sh
# requires Rscript on PATH
cargo test -p ry-checker --test oracle -- --ignored --nocapture
```

Each fixture's first line declares its expectation:

- `# oracle: must-pass` -- R succeeds; ry must emit no error.
- `# oracle: must-flag` -- R errors; ry must emit at least one error.
- `# oracle: known-gap <one-line reason>` -- ry and R are expected to
  disagree today. The harness prints the delta but does not fail; it DOES
  fail if the gap unexpectedly closes (a stale tag) so gaps can't ship red.

## License

MIT OR Apache-2.0.
