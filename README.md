
<!-- README.md is generated from README.Rmd. Edit README.Rmd, then
     render with the release binary on PATH:
       PATH="$PWD/target/release:$PATH" Rscript -e 'rmarkdown::render("README.Rmd")'
     Chunks that invoke `ry` run only when the binary is available, so
     the output below is real, not hand-written. -->

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

## Who this is for

ry is built to support **education, practice, and research around
principled Bayesian workflows** in R:

  - **Education.** Every diagnostic explains what R would silently do at
    runtime, so the checker doubles as a teacher of R’s semantics. A
    docs site, worked Bayesian-workflow demos, and graded exercises are
    in progress – see [ROADMAP.md](ROADMAP.md).
  - **Practice.** ry understands the modern parallel idiom –
    `purrr::in_parallel()` on `mirai` daemons – and type-checks parallel
    code exactly like its sequential equivalent.
  - **Research.** Diagnostics are available as structured JSON
    (`--output-format json`) with per-rule counts (`--statistics`), so
    ry can instrument code quality across a corpus or a classroom. How
    ry validates its own claims against R is described in
    [ARCHITECTURE.md](ARCHITECTURE.md).

ry is **not** a formatter (pair it with `air` or `styler`) and not a
replacement for `lintr`’s style rules. It focuses on type- and
scope-driven diagnostics that need a whole-program view, and it prefers
staying silent over crying wolf: every rule is validated against an R
oracle and real CRAN code so that a warning from ry is worth reading.

## Install

ry is a Cargo workspace; build from source (Rust 1.82 or newer):

``` sh
git clone https://github.com/sims1253/ry
cd ry
cargo build --release
# binary at target/release/ry
```

Prebuilt binaries and an R-side installer are on the
[roadmap](ROADMAP.md).

## Quickstart

Point `ry check` at files, directories, or a project root (`.R` and `.r`
files are collected recursively; Quarto/R Markdown chunk checking is on
the roadmap):

``` bash
ry check examples/smoke.R
#> examples/smoke.R:10:6: error: [RY040] cannot apply arithmetic op to `character` and `integer`
#>   y <- "a" + 1L
#>        ^~~~~~~~
#> examples/smoke.R:16:5: warning: [RY001] `if` condition is `character` (not logical); will be silently coerced
#>   if ("x") print(nums)
#>       ^~~
#> examples/smoke.R:19:5: warning: [RY002] `if` condition has length 2, will only use first element
#>   if (c(TRUE, FALSE)) print(1)
#>       ^~~~~~~~~~~~~~
#> examples/smoke.R:27:6: warning: [RY010] variable `undefined_thing` is not bound in this scope
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

## Package awareness

ry tracks `library()` / `require()` calls and resolves functions against
per-package type stubs, so the same name means the right thing in
context: `filter()` is `stats::filter` until dplyr is loaded, and
`dplyr::filter(df, x > 0)` resolves the column `x` against `df`’s schema
either way. Stubs currently ship for base R (plus stats and utils),
dplyr, purrr, mirai, and a minimal Bayesian stack (brms, posterior, loo,
bayesplot, cmdstanr). Packages attached outside the checked sources can
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

The table below is generated from the checker’s own rule registry at
render time, so it cannot drift from the binary. Defaults can be
overridden per-project; `ry rule RY040` prints the explanation for one
rule.

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
| RY040 | invalid-arithmetic       | error    | Arithmetic operator between incompatible types.                                                                                                                                                           |
| RY050 | missing-s3-method        | warning  | S3 generic called on a value with no defined method for its class.                                                                                                                                        |
| RY060 | undefined-column         | error    | Column access on a value whose schema does not contain that column.                                                                                                                                       |
| RY061 | dollar-on-atomic         | error    | The $ operator is invalid for atomic vectors (integer, double, character, logical). It only works on list-like types (lists, data frames, environments).                                                  |
| RY070 | call-non-function        | error    | A non-function value (a variable bound to a non-function, or a literal like `42()`) is being called as a function. R will error at runtime (‘attempt to apply non-function’ / ‘could not find function’). |
| RY080 | map-return-type-mismatch | warning  | A purrr typed-map (`map_dbl`, `map_int`, …) callback returns a value whose mode is incompatible with the target vector type. R coerces at runtime, but the mismatch is almost always unintended.          |

## How ry stays honest

  - **The R oracle.** Every behavioral claim is checked against R
    itself: the fixture corpus is executed by an R driver – in parallel,
    via `purrr::in_parallel()` on mirai daemons, the same idiom the
    checker models – and ry must agree with R’s verdict. Runs in CI;
    `cargo test -p ry-checker --test oracle -- --ignored` locally.
  - **Vendored CRAN code.** ry runs over vendored real-world packages
    (currently glue and purrr) and snapshots every diagnostic; each
    snapshot line is triaged, and a rule that dominates a snapshot is
    treated as broken. The glue snapshot is empty.
  - **A typeshed that cannot invent functions.** Every name in the
    embedded type stubs is verified against R in CI
    (`scripts/audit_typeshed.R`).

Known gaps: no S4 / R6 / environment modeling, no NAMESPACE resolution
(cross-package names outside the shipped stubs resolve to opaque), no NA
tracking yet. See [ROADMAP.md](ROADMAP.md).

## Contributing and architecture

Start with [ARCHITECTURE.md](ARCHITECTURE.md) (crate map, the three-pass
project check, where to add a rule) and
[CONTRIBUTING.md](CONTRIBUTING.md) (build gate, fixture conventions, and
the false-positive bar every new rule must clear).

## License

MIT OR Apache-2.0.
