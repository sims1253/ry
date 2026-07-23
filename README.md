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
#> demo.R:4:5: warning: [RY002] `if` condition has length 2; R requires a length-1 condition
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
in `NAMESPACE` preserve exact binding provenance, falling back to opaque when
ry has no stub for the dependency. Whole-package imports and `library()` / `require()` calls also
use installed packages' static `NAMESPACE` exports without executing R or
loading package code. `requireNamespace()` deliberately does not introduce
unqualified names.

Stubs are maintained in the standalone
[r-typeshed](https://github.com/sims1253/r-typeshed) repository and
vendored into the binary. They currently cover base R (with a
mechanically generated symbol inventory for the default packages),
the tidyverse core (dplyr, tidyr, tidyselect, dbplyr, purrr), the
Bayesian stack (brms, posterior, loo, bayesplot, cmdstanr), and
testthat, tinytest, withr, R6, S7, Rcpp, foreach, shiny, survival,
recipes, mirai, and others. Packages attached outside the checked
sources can be declared in `ry.toml`.

When checking a package source tree, files are checked in their
evaluation context: `tests/testthat/` and `inst/tinytest/` files see
the package’s own namespace, the test framework, DESCRIPTION
`Depends` / `Suggests`, and bindings plus `library()` calls from
`helper*` / `setup*` files; `data-raw/`, `demo/`, and `vignettes/`
attach `Depends`. `revdep/`, `src/`, snapshot data, and
`.Rbuildignore` matches (never `R/` or `tests/`) are skipped.

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

## Data masking and NSE

Stubs declare which parameters are data-masked, tidy-selected, or
quoted (for tidyverse packages this metadata is generated from the
`<data-masking>` / `<tidy-select>` markers in their documentation).
Columns inside a masked argument resolve against the data frame's
schema instead of the lexical scope:

``` bash
ry check nse.R
#> nse.R:4:23: warning: [RY010] variable `mgp` is not bound in this scope
#>   summarise(d, m = mean(mgp))
#>                         ^~~
#> ry: checked 1 file(s), 0 error(s), 1 warning(s)
```

where `nse.R` is

``` r
library(dplyr)
d <- data.frame(mpg = c(21, 22.8), cyl = c(6, 4))
summarise(d, m = mean(mpg))                                # resolves
summarise(d, m = mean(mgp))                                # typo, caught
my_mean <- function(df, var) summarise(df, m = mean({{ var }}))  # silent
```

rlang’s `{{ }}` embrace, the `.data` / `.env` pronouns, `!!` / `!!!`,
and functions that defuse their own arguments (a parameter whose first
use is `enquo()` / `substitute()` / …) are recognized, so wrapper
functions do not produce false unbound-variable reports. When the
masked data’s schema is unknown, column candidates stay silent rather
than guessed at.

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

# Names created dynamically by the host application or an unresolvable
# load(). Only these names are treated as opaque globals.
globals = ["runtime_data", "generated_lookup"]

# Additional package stubs. Paths are relative to this ry.toml.
typeshed = ["stubs", "../shared-r-stubs"]

# Accepted findings from `ry check --write-baseline`; new findings still fail.
baseline = "ry-baseline.json"

error-on-warning = false
exit-zero        = false
output-format    = "full"     # full | concise | json | github | gitlab | junit

# gitignore-style patterns, relative to this ry.toml's directory.
exclude = ["renv", "tests/snaps/**"]
```

CLI flags override the config only when passed explicitly.
When multiple paths are checked, the first path anchors config discovery;
that one configuration applies to the complete invocation.

## Custom typesheds

Custom stub directories let a project add package signatures or replace ry's
vendored signatures without recompiling. Both `stubs/foo.json` and
`stubs/foo/foo.json` layouts are accepted. The optional `package` header names
the package; legacy files fall back to the JSON file stem.

Directories are layered in declaration order and `--typeshed <DIR>` may be
repeated to append CLI directories. Later directories win, so CLI stubs replace
same-named config stubs. A custom package replaces the embedded package as a
whole; function-by-function merging is intentionally not performed. A
`base.json` stub likewise replaces the embedded base typeshed for that run.
Malformed files produce a warning naming the file while valid siblings remain
active.

Run `ry explain typeshed` to see the vendored snapshot, embedded packages, and
the custom directories active from the current workspace's `ry.toml`.

## Inline suppression

``` r
x <- bad  # ry: ignore                 # suppress all rules on this line
x <- bad  # ry: ignore[RY010, RY040]   # suppress specific rules
x <- bad  # noqa: RY010                # flake8/ruff-compatible alias

# ry: ignore                           # standalone: suppresses the next line
# ry: ignore-file                      # file-level, anywhere in the file
```

Prefer a rule-specific inline suppression or `globals` entry for dynamic
workspaces. ry intentionally does not suppress diagnostics merely because an
expression appears inside `expect_error()`: the setup expression is ordinary R
code and can contain a real defect before the expected error is reached.

## Confidence tiers and baselines

Every diagnostic carries a confidence tier. Structurally exact rules
(RY093, RY094, RY096, RY030, RY033, …) are `high`; RY010 is `medium`;
diagnostics from `tests/`, `data-raw/`, `demo/`, `vignettes/`, and
`inst/` are demoted one tier. Output is sorted by tier, non-medium
tiers are tagged in the message, and `--min-confidence high|medium|low`
filters both output and exit code.

To adopt ry on an existing codebase, snapshot the current findings and
fail only on new ones:

``` bash
ry check --write-baseline ry-baseline.json .
ry check --baseline ry-baseline.json .    # or `baseline` in ry.toml
```

Baseline entries match on path, rule, and message – not line numbers –
so unrelated edits do not invalidate them. Fixed findings can be
removed by regenerating the baseline.

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

Defaults can be overridden per-project; `ry explain rule RY040` prints the
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
| RY090 | unknown-argument         | warning  | A named call argument does not match any formal parameter after R’s exact and partial argument matching.                                                                                                  |
| RY091 | missing-required-argument | warning | A required formal parameter is not bound by name or position.                                                                                                                                             |
| RY092 | argument-type-mismatch   | error    | A call argument has a known mode incompatible with the parameter type declared by the resolved signature.                                                                                                 |
| RY093 | comparison-inside-length | warning  | A comparison directly inside `length()` (also `nchar()`, `abs()`) is usually a parenthesization mistake.                                                                                                  |
| RY094 | printf-argument-count    | warning  | A literal printf-family format string has more conversions than supplied value arguments.                                                                                                                 |
| RY096 | hasarg-non-formal        | warning  | `hasArg()` names a parameter that is not a formal of an enclosing function without `...`.                                                                                                                 |
| RY097 | not-r-source             | info     | File does not appear to be R source (e.g. Ratfor); its diagnostics are suppressed.                                                                                                                        |
| RY098 | default-forced-before-assignment | warning | A parameter default references a body-local that may not be assigned yet on some execution path.                                                                                                    |
| RY099 | discarded-conditional-value | warning | A value-producing expression in a non-tail one-arm `if` is discarded, commonly because an assignment was omitted.                                                                                   |
| RY100 | comparison-inside-math-call | warning | A comparison directly inside a numeric math function is usually a parenthesization mistake.                                                                                                         |
| RY101 | identical-list-subset-scalar | warning | `identical()` compares a single-bracket list subset with an atomic scalar, making the result always `FALSE`; use `[[` to extract the element.                                                        |

Known gaps: S4 modeling covers in-package `setClass` / `setGeneric` /
`setMethod` and `@` slot access but not full method resolution order;
R6 modeling covers `self` / `private` / `super` in method bodies, not
field types. No expansion of dynamic `exportPattern()` directives and no
NA tracking yet. Cross-package names without stubs resolve to opaque
values when static package metadata proves that they exist.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the build gate, fixture
conventions, and the false-positive bar every new rule must clear.
