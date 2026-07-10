# ry

[![CI](https://github.com/sims1253/ry/actions/workflows/ci.yml/badge.svg)](https://github.com/sims1253/ry/actions/workflows/ci.yml)

`ry` is a fast static checker for the R language, written in Rust and
inspired by [astral-sh/ty](https://github.com/astral-sh/ty). It parses R
source with [tree-sitter-r](https://github.com/r-lib/tree-sitter-r),
infers types across your whole project, and reports likely bugs before
you run the code: calling a non-function, arithmetic on incompatible
types, misspelled data frame columns, unbound variables, malformed `if`
conditions, and more.

R will happily coerce, recycle, and partially match its way past most of
these mistakes at runtime and hand you a statistically wrong answer
instead of an error. ry’s job is to catch the mistake while it is still
cheap.

ry is **not** a formatter (pair it with `air`) and not a
replacement for `lintr`’s style rules. It focuses on type- and
scope-driven diagnostics that need a whole-program view.

## Install

ry is a Cargo workspace; build from source (Rust 1.86 or newer):

``` sh
git clone https://github.com/sims1253/ry
cd ry
cargo build --release
# binary at target/release/ry
```

Prebuilt binaries are attached to GitHub releases.

## Quickstart

Point `ry check` at files, directories, or a project root (`.R` and `.r`
files are collected recursively):

``` bash
cat > demo.R <<'EOF'
nums <- 1:3
y <- "a" + 1L
if ("x") print(nums)
if (c(TRUE, FALSE)) print(1)
z <- undefined_thing
EOF

ry check demo.R
#> demo.R:2:6: error: [RY040] cannot apply arithmetic op to `character` and `integer`
#>   y <- "a" + 1L
#>        ^~~~~~~~
#> demo.R:3:5: warning: [RY001] `if` condition is `character` (not logical); will be silently coerced
#>   if ("x") print(nums)
#>       ^~~
#> demo.R:4:5: warning: [RY002] `if` condition has length 2, will only use first element
#>   if (c(TRUE, FALSE)) print(1)
#>       ^~~~~~~~~~~~~~
#> demo.R:5:6: warning: [RY010] variable `undefined_thing` is not bound in this scope
#>   z <- undefined_thing
#>        ^~~~~~~~~~~~~~~
#> ry: checked 1 file(s), 1 error(s), 3 warning(s)
```

Diagnostics use the `full` format by default – the offending line with
the span underlined – and messages carry the context you need to act,
e.g. a column miss lists what IS there:

``` bash
ry check /tmp/ry-readme/analysis.R
#> /tmp/ry-readme/analysis.R:2:11: error: [RY060] column `dispp` not found in data frame schema; available columns: mpg, disp
#>   m <- mean(d$dispp)
#>             ^~~~~~~
#> ry: checked 1 file(s), 1 error(s), 0 warning(s)
```

Exit codes are CI-friendly: non-zero when any error-level diagnostic
fires (`--exit-zero` overrides; `--error-on-warning` promotes warnings).
Human-readable diagnostics use ANSI color on terminals; select the policy
explicitly with `--color auto|always|never`. Automatic color respects
`NO_COLOR`, and machine-readable formats never contain ANSI escapes.

## Package awareness

ry tracks `library()` / `require()` calls and resolves functions against
per-package type stubs, so the same name means the right thing in
context: `filter()` is `stats::filter` until dplyr is loaded, and
`dplyr::filter(df, x > 0)` resolves the column `x` against `df`’s schema
either way. For packages being checked, `importFrom(pkg, name)` directives
in `NAMESPACE` introduce opaque bindings even when ry has no stub for the
dependency. Whole-package imports and `library()` / `require()` calls also
use installed packages' static `NAMESPACE` exports without executing R or
loading package code. `requireNamespace()` deliberately does not introduce
unqualified names.

Stubs currently ship for base R (plus stats and utils),
dplyr, purrr, mirai, survival, and a minimal Bayesian stack (brms,
posterior, loo, bayesplot, cmdstanr). Packages attached outside the checked sources can
be declared in `ry.toml`.

Parallel purrr code checks like sequential code, and the typed map
family is checked against its callback:

``` bash
ry check /tmp/ry-readme/parallel.R
#> /tmp/ry-readme/parallel.R:5:8: warning: [RY080] `map_dbl` expects `double` returns but the callback returns `character`; R will coerce silently
#>   bad <- map_dbl(1:4, function(i) as.character(i))
#>          ^~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
#> ry: checked 1 file(s), 0 error(s), 1 warning(s)
```

`in_parallel()` is type-transparent to ry: a `map_dbl` whose callback
returns character is a diagnostic before the run, not a surprise halfway
through your simulation study.

## Configuration (`ry.toml`)

Discovered by walking up from the checked path. All keys optional:

``` toml
# Promote / demote / disable rules by code (RY040), name
# (invalid-arithmetic), or "all".
error  = ["RY040"]
warn   = ["RY070"]
ignore = ["RY033"]

# Packages attached outside the checked sources.
packages = ["dplyr"]

error-on-warning = false
exit-zero        = false
output-format    = "full"     # full | concise | json | github | gitlab | junit

# gitignore-style patterns, relative to this ry.toml's directory.
exclude = ["renv", "tests/snaps/**"]
```

CLI flags override the config only when passed explicitly.
When multiple paths are checked, the first path anchors config discovery;
that one configuration applies to the complete invocation.

## Inline suppression

``` r
x <- bad  # ry: ignore                 # suppress all rules on this line
x <- bad  # ry: ignore[RY010, RY040]   # suppress specific rules
x <- bad  # noqa: RY010                # flake8/ruff-compatible alias

# ry: ignore                           # standalone: suppresses the next line
# ry: ignore-file                      # file-level, anywhere in the file
```

## Editors

`ry server` speaks the Language Server Protocol over stdio: diagnostics
as you type (debounced, cached parses), hover with inferred types,
go-to-definition, references, rename, completion, signature help, inlay
hints, folding, and quick-fix actions that insert suppression comments.
Connect it from any LSP-aware editor (VS Code, Positron, Neovim, Helix,
…).

## CI

`--output-format github` emits workflow-command annotations; `gitlab`
and `junit` cover the other major CI systems. `--statistics` prints
per-rule counts after a run – useful for corpus work. A minimal GitHub
Actions step:

``` yaml
- run: ry check --output-format github .
```

## Rules

Defaults can be overridden per-project; `ry rule RY040` prints the
explanation for one rule.

| code  | name                     | severity | summary                                                                                                                                                                                                   |
| :---- | :----------------------- | :------- | :-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RY000 | syntax-error             | error    | Unparseable input. tree-sitter could not recover this region; subsequent diagnostics may be unreliable.                                                                                                   |
| RY001 | invalid-condition        | warning  | `if` / `while` condition is not a length-1 logical.                                                                                                                                                       |
| RY002 | condition-length         | warning  | `if` condition length is known to be greater than 1; only the first element is used.                                                                                                                      |
| RY010 | unbound-variable         | warning  | Reference to a variable with no binding in scope.                                                                                                                                                         |
| RY020 | unary-minus-type         | error    | Unary `-` applied to a non-numeric type.                                                                                                                                                                  |
| RY021 | unary-not-type           | error    | Unary `!` applied to a non-coercible-to-logical type.                                                                                                                                                     |
| RY030 | invalid-comparison       | error    | Comparison between types with no defined ordering.                                                                                                                                                        |
| RY031 | invalid-logical-op       | error    | `&` / `&#124;` / `&&` / `&#124;&#124;` applied to non-coercible types.                                                                                                                                    |
| RY032 | scalar-logical-length    | warning  | `&&` and `&#124;&#124;` only use the first element of their operands; using them with vectors of length \> 1 is almost always a bug. Use `&`/`&#124;` for vectorized operations.                          |
| RY033 | comparison-mode-mismatch | warning  | Comparing a character value with a numeric value is valid R but almost always unintended. R compares byte values, not semantic equality.                                                                  |
| RY034 | compare-na               | warning  | Comparing with `NA` using `==` or `!=` always produces `NA`. Use `is.na()` instead.                                                                                                                       |
| RY040 | invalid-arithmetic       | error    | Arithmetic operator between incompatible types.                                                                                                                                                           |
| RY041 | non-divisible-recycling  | warning  | Vector lengths do not divide evenly, so R recycles values with a warning and may produce unintended results.                                                                                             |
| RY042 | factor-arithmetic        | warning  | Arithmetic on factors produces missing values. Operate on levels or convert explicitly.                                                                                                                  |
| RY050 | missing-s3-method        | warning  | S3 generic called on a value with no defined method for its class.                                                                                                                                        |
| RY060 | undefined-column         | error    | Column access on a value whose schema does not contain that column.                                                                                                                                       |
| RY061 | dollar-on-atomic         | error    | The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).                                                  |
| RY070 | call-non-function        | error    | A non-function value (a variable bound to a non-function, or a literal like `42()`) is being called as a function. R will error at runtime (‘attempt to apply non-function’ / ‘could not find function’). |
| RY080 | map-return-type-mismatch | warning  | A purrr typed-map (`map_dbl`, `map_int`, …) callback returns a value whose mode is incompatible with the target vector type. R coerces at runtime, but the mismatch is almost always unintended.          |

Known gaps: no S4 / R6 / environment modeling, no expansion of dynamic
`exportPattern()` directives, and no NA tracking yet. Cross-package names
without stubs resolve to opaque values when static package metadata proves
that they exist.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the build gate, fixture
conventions, and the false-positive bar every new rule must clear.
