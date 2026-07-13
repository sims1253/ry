# Changelog

All notable changes to ry are documented in this file.

## [0.4.0] - 2026-07-13

Precision release driven by the top-300 CRAN audit: the corpus total fell
from ~23,300 diagnostics to ~6,500 (-72%) while every confirmed real bug
in the audit's regression list still surfaces, and the new rule family
found previously unknown bugs (scales `!length(x) == 1` guards among
them).

### Added

- Typed and required parameter metadata in typeshed signatures, including
  numeric mode unions and strict validation through `ry typeshed validate`.
- R-compatible exact, partial, and positional call-argument matching with
  `RY090` for unknown named arguments, `RY091` for missing required arguments,
  and `RY092` for provable argument type mismatches.

- Runtime custom typeshed loading through the `typeshed` key in `ry.toml` and
  repeatable `--typeshed` flags. Flat and nested stub layouts are supported,
  later directories replace earlier packages, and editor diagnostics use the
  same workspace configuration.
- The embedded typeshed is now a vendored snapshot of the standalone
  `r-typeshed` repository, with schema-version validation and source metadata.
- New mis-parenthesization rule family: `RY093` (comparison inside
  `length()`/`nchar()`/`abs()`, also detected inside `&&`/`||` operands),
  `RY095` (`!x == y` negation-comparison precedence), and `RY096`
  (`hasArg()` naming a non-formal of the enclosing function).
- `RY094`: printf-family (`sprintf`/`gettextf`) literal format strings are
  checked against the supplied argument count.
- `RY097`: files whose top-level statements are mostly unparseable (Ratfor
  sources, broken fixtures) collapse into a single info diagnostic instead
  of hundreds of spurious errors.
- `RY098`: a parameter default referencing a body-local is flagged when an
  execution path can force the default before the local is assigned;
  the idiomatic late-bound default stays silent.
- Confidence tiers: every diagnostic carries `high`/`medium`/`low`
  confidence, output is ranked by tier, diagnostics under `tests/`,
  `data-raw/`, `demo/`, `vignettes/`, and `inst/` are demoted one tier, and
  `--min-confidence` filters both output and exit code. A symbol used in
  value position that only resolves to a function from another namespace is
  reported at high confidence with the resolution target in the message.
- Baseline workflow for incremental adoption: `ry check --write-baseline`
  snapshots current diagnostics (line-number-free matching) and
  `--baseline` / the `baseline` config key subtracts them from later runs.
- Package-aware scan contexts: `tests/testthat/` files see the package's
  own namespace, `testthat`, DESCRIPTION `Depends`/`Suggests`, and
  `helper-*.R`/`setup-*.R` bindings; `data-raw/`, `demo/`, and `vignettes/`
  attach `Depends`; `.Rbuildignore` patterns (Perl regexes) are respected
  without ever excluding `R/` or `tests/`.
- NSE completion: rlang `{{ }}` embrace is recognized as a mask escape
  (typos inside it still flagged), and the `.data$col` / `.data[["col"]]` /
  `.env$var` pronouns resolve against the mask schema or lexical scope.
- Minimum-viable S4 modeling: in-package `setClass`/`setGeneric`/
  `setMethod` are collected across files and dispatched on receiver class,
  `@` slot access is modeled, and vector names survive `t()` and
  `data.frame()` construction.
- Scope and flow fixes: `inherits(x, "cls")` guards narrow types,
  `useDynLib(.fixes=)` prefixes resolve native-routine symbols, R6/S7
  method bodies see `self`/`private`/`super`, top-level
  `assign(..., envir = asNamespace(...))` binds, and replacement-function
  assignments (`dimnames<-` and friends) keep the target bound.
- User-defined infix operators (`%op%`) preserve their operands in the AST;
  zeallot/future `%<-%`/`%->%` destructuring introduces its pattern
  bindings when a package defining the operator is in scope.
- Data-driven semantics via new `injects` stub metadata: `withr::with_*`
  path injection and R6/S7 method-environment bindings now come from the
  typeshed instead of hardcoded checker logic.
- Derived NSE for user-defined functions: a parameter whose first use is a
  defusing call (`enquo`, `enexpr`, `ensym`, `quo`, `substitute`,
  `match.call`, ...) marks call-site arguments as unevaluated, so
  arrow-style test helpers (`compare_dplyr_binding(.input %>% ...)`) stop
  producing unbound-variable noise.
- testthat helper/setup files now propagate their `library()`/`require()`
  attachments (not just bindings) to test files, and the helper filename
  match covers all `helper*`/`setup*` prefixes.
- The data-mask gate is fully data-driven: any loaded package whose stub
  declares `eval` metadata gets NSE treatment (rlist, patrick, bench, ...),
  and user-defined S3 methods inherit the eval metadata of a stubbed
  generic with the same name (dtplyr/dbplyr verb methods).
- `foreach(i = ..., p = ...) %do%/%dopar%/%op% { ... }` binds the loop
  variables in the body regardless of the operator alias used.
- `attach(x)` marks the scope's search path as unanalyzable, silencing
  unbound-variable diagnostics for legacy attach-style scripts.
- Type narrowing applies to expression-position `if` (e.g.
  `x <- if (is.function(f)) f(1) else f`).
- Tidyverse NSE metadata is now GENERATED from installed-package Rd docs
  (`gen_nse_metadata.R` in r-typeshed reads the `<data-masking>` /
  `<tidy-select>` argument markers), giving full dplyr/tidyr coverage and
  a new tidyselect stub; dynamically registered S3 methods inherit their
  generic's NSE metadata.
- `.` binds inside data-masked arguments (dplyr `do()`, pipe idioms), for
  both `%>%` and the native `|>` pipe.
- Defused-parameter derivation covers `{{ }}` embrace usage and exclusive
  `enquos(...)`-style `...` defusal in user functions.
- Inside a data-masked argument with an unknown schema, lexically resolved
  symbols infer as opaque — mask columns may shadow them, so their lexical
  types no longer drive arithmetic/comparison diagnostics.
- Rcpp modeled as a first-class package: `sourceCpp()` carries the new
  `scope_effect: unknown_bindings` stub metadata (compiled exports are
  unknowable), `cppFunction()` returns a function, and `base::attach` now
  uses the same data-driven mechanism instead of a hardcoded recognizer.
- tinytest scan context: files under `inst/tinytest/` see the package's
  own namespace, `tinytest`, and DESCRIPTION Depends/Suggests, mirroring
  the testthat context.

### Changed

- `RY_NO_INSTALLED_LIBRARIES=1` disables resolution of imported-package
  exports from the machine's R installation; the ecosystem regression
  harness sets it so committed snapshots are environment-independent.
- The ecosystem harness report writer is implemented in R (jsonlite)
  instead of python3; the harness now requires `Rscript`.
- Typeshed auditing and stub generation removed from this repository's CI
  and scripts — they live in r-typeshed, whose CI runs them.

## [0.3.0] - 2026-07-11

This release focuses on diagnostic precision, driven by audits of five
real-world CRAN packages (brms, posterior, bayesplot, loo, cmdstanr).
Total diagnostics across those clean corpora dropped from 837 (240
errors) to 30 (1 error — a genuine bug in brms), while all genuine
findings from the audits are still reported. The largest corpus checks
in under a second in release mode.

### Added

- S3 dispatch for operators: binary and unary `Ops` group methods
  (including operator-specific methods such as `+.classname`) defined in
  the checked sources or an attached package are now consulted before
  arithmetic and comparison diagnostics.
- Data-frame schema tracking: `data.frame()` derives column names from
  positional expressions (`data.frame(y, K)` has columns `y` and `K`),
  and column writes via `$`, `[[`, and partial indexed assignment update
  the tracked schema.
- Static dataset inventory: bindings introduced by a package's `data/`
  directory and by `load()` of a project `.rda`/`.RData` file are
  resolved by reading only the top-level tags of the R serialization
  stream (gzip, bzip2, and xz supported) — no R code is executed.
- NSE evaluation modes in typeshed stubs: parameters can be declared as
  data-masked, tidy-select, or quoted, so dplyr-style verbs resolve
  columns instead of flagging them as undefined globals.
- Typeshed stubs for testthat, plus expanded base, Bayesian-stack, and
  dplyr catalogues; stubs can also declare source-relative path
  arguments so `source("helper.R")`-style calls are followed.
- `globals` key in `ry.toml` for names created dynamically by the host
  application or an unresolvable `load()`; only the listed names become
  opaque, without suppressing other diagnostics.
- Lexical closure capture: names assigned anywhere in enclosing function
  bodies are visible inside nested closures, matching R's deferred
  lookup, without making direct read-before-assignment valid.
- Forwarded-default analysis: a formal forwarded into another function
  is credited with the callee's reachable defaults, removing false
  `NULL`-default condition warnings while keeping the genuine ones.

### Fixed

- `importFrom(pkg, name)` now preserves exact binding provenance when a
  stub for the dependency exists, falling back to opaque otherwise.
- Numeric truthiness idioms (e.g. `if (length(x))`) and list/atomic
  equality comparisons no longer produce false diagnostics.
- Various false positives around class-attribute assignment, nested
  record-path writes, S3 predicate narrowing, and dplyr join calls.

## [0.2.0] - 2026-07-10

### Added

- Static resolution of `NAMESPACE` imports, including
  `importFrom(package, name)` and whole-package imports.
- Resolution of exports introduced by `library()` and `require()` without
  executing R or loading package code.
- Support for installed package libraries on Linux, macOS, Windows, and
  renv-managed projects.
- ANSI-colored human-readable diagnostics with
  `--color auto|always|never` and `NO_COLOR` support.
- `RY034` for comparisons with `NA` using `==` or `!=`.
- `RY041` for non-divisible vector recycling.
- `RY042` for arithmetic on factors.

### Fixed

- False-positive `RY010` diagnostics for imported package values such as
  bare `tags` imported from shiny.
- `requireNamespace()` no longer incorrectly introduces unqualified names.
- Package bindings no longer leak between unrelated packages checked together.
- Package-library and R-version precedence now respect the active project,
  including renv libraries.
- Several arithmetic, raw-vector, factor-comparison, assignment, and scope
  inference edge cases.

### Changed

- The minimum supported Rust version is now 1.88 and is verified in CI.
- Human and machine-readable diagnostic output are tested independently;
  JSON and CI formats never contain ANSI escapes.

## [0.1.0] - 2026-07-07

- Initial release.

[Unreleased]: https://github.com/sims1253/ry/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/sims1253/ry/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/sims1253/ry/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sims1253/ry/releases/tag/v0.1.0
