# Changelog

All notable changes to ry are documented in this file.

## [0.6.0] - 2026-07-17

Driven by the ry 0.5.0 top-500 CRAN audit (9,237 diagnostics, 1.55%
precision) and a subsequent generalization pass. On the same 504-package
corpus this release emits 3,442 diagnostics (-63%; -69% counting only
warnings/errors), with every previously cataloged true positive either
preserved or its loss individually adjudicated, and ~10 new real shipped
bugs found by the new RY100 rule. Minor bump: scope resolution, rule
routing (RY001/RY003), and quoting semantics intentionally change
reported diagnostics.

### Added

- RY003 `numeric-condition` (Info): numeric `if`/`while` conditions are
  legal, idiomatic R (`if (nchar(x))`, `if (n)`); they are no longer
  RY001 warnings. RY001 keeps the genuinely erroneous modes (character,
  list, NULL, function, length-0).
- RY100 `comparison-inside-math-call` (Warning): a comparison directly
  inside `abs`/`sqrt`/`exp`/`log*`/`floor`/`ceiling`/`round`/`trunc` is
  almost always a parenthesization slip (`abs(x > y)` for `abs(x) > y`).
  Generalizes RY093, ry's highest-precision rule; corpus census found 10+
  real shipped bugs (effects, ggplot2 tests, performance, pracma) at ~100%
  precision after excluding the deliberate `sign(cmp)` indicator idiom.
- RY040 fires on arithmetic with a known-NULL operand (`x / NULL` is
  `numeric(0)`), gated to literal NULLs and missing fields of complete,
  locally built `list(...)` schemas so parameter defaults never trip it.
- Environment profiles: files sourced into a known framework context get
  its ambient bindings. Shiny app trees (`input`/`output`/`session`) ship
  built in; users declare their own via `[[environments]]` in `ry.toml`
  (`name`, `bindings`, `paths`).
- `ry.toml` `max-serialized-bytes` (default 2 MiB) caps `.rda` workspace
  enumeration; oversized workspaces open the file's scope instead of
  stalling the scan (bigD: 190 s -> 0.13 s).
- File collection accepts the full R source extension set (`.S`, `.s`,
  `.q` — boot's entire library was previously invisible), decodes Latin-1
  sources instead of skipping them, and skips `*.Rcheck` build artifacts.

### Scope and name resolution

- `library()`/`require()` of a package without a stub marks the search
  path unknown, silencing RY010 for names that plausibly come from it —
  the single largest false-positive source in the audit (lazy-loaded
  datasets such as `sleepstudy`, `apipop`). Stubbed packages keep full
  checking. `data()`/`load()`/`source()`/`sys.source()` declare the same
  effect via stub metadata; `data(x)` also binds its literal names.
- Attachment is context-scoped to match R's semantics: package `R/` code
  resolves bare names against base plus exactly what NAMESPACE grants
  (`importFrom` names, wholesale `import(pkg)` exports); test and script
  files resolve against the testthat runner world (testthat, the package
  under test, helper/setup and in-file `library()` calls, and DESCRIPTION
  Suggests). Imports no longer leak whole-package exports into files that
  never attached them (arrow's `string`/`int`/`dbl` vs rlang).
- Loop bodies pre-bind names assigned anywhere in the body, so
  loop-carried accumulators read before their first syntactic assignment
  no longer fire RY010.
- `on.exit(expr)` is checked against exit-time bindings (everything the
  function assigns), not walk-order bindings.

### NSE and quoting

- User functions that quote their arguments are detected from their
  bodies — `substitute`/`match.call`/`sys.call`/`bquote` and, via stub
  metadata, the rlang capture family (`enquo`, `enexpr`, `ensym`, plural
  forms, `quos`) — and the property propagates: through direct argument
  forwarding between user functions, from stub eval modes into user
  wrappers, and from S3 methods onto their generics (named method params
  absorbed by the generic's `...` included). lambda.r: 165 -> 0 RY010;
  sparklyr: 93 -> 0.
- Quoted arguments receive no diagnostics at all — they are data, not
  code (igraph's `graph_from_literal(A +-+ B)` no longer type-errors).
- Operands of unknown `%op%` infix operators and unresolvable `.()`
  calls are treated as quoted.
- Formula-interface arguments (`weights`, `subset`, `offset`, `id`,
  `cluster`, `istate`) evaluate inside the `data` mask via the new
  `data_mask_source` stub metadata (stats and survival interfaces).
- String-literal calls (`"paste"(1, 2)`, `"[<-.data.frame"(...)`) resolve
  like identifiers instead of firing RY070; character *variables* in call
  position still do.

### Type system

- Divergence-aware narrowing: a guard whose branch always exits
  (`if (is.null(x)) stop(...)`, `return`, `abort` via the new `no_return`
  stub property, `if (!length(x)) return(...)`) narrows the continuation.
  Never-returning user helpers are detected recursively; a project-local
  function named `abort` is not assumed to diverge.
- Narrowing-installed bindings are tracked explicitly, so a real
  assignment inside a branch always overrides a temporary refinement in
  the post-if merge (fixes stale-NULL cascades through the cross-file
  fixpoint).
- `df[, j]` single-column selection honors `drop = TRUE` (a parser fix:
  the empty row index was previously dropped entirely) and returns the
  column type; scalar subscripts narrow to length 1; negative literals
  keep vector length.
- S3 dispatch walks the full class vector across all method sources;
  `Ops`/`Math`/`Summary` group generics dispatch for data.frames and user
  classes (`df / 2`, `ggplot() + NULL`-style idioms); RY050 fires only
  for generics the project itself demonstrably owns.
- `list(...)` containing dots yields an incomplete schema — a missing
  field is no longer known-NULL; `$`/`[[` through a parameter whose only
  evidence is an overridable NULL default yields unknown.
- A condition typed as a union with at least one valid length-1 logical
  member is not reported (only provably invalid unions are).
- `append()` returns the concatenation of its arguments; `tapply` gained
  a higher-order simplify spec; `mapply` honors `SIMPLIFY = TRUE`
  (all stub-data fixes, vendored from r-typeshed 0.2.0 along with new
  rlang and cli stubs).

### Fixed

- Panic (`index out of bounds`) in quoting-forwarding when a user callee
  and a stub callee had different parameter counts; it crashed scans of
  17 corpus packages (psych, rlang, recipes, …).
- `readLines()` no longer demands `con` (stub had it wrongly required);
  a generator-level fix detects `missing()`-based optionality so the
  whole class (`rlang::env_get(default=)`) cannot recur.
- RY033's stale-type false positives after both `if`/`else` arms rebind a
  variable.
- RY100 subsumes the condition-type diagnostic on the same span (no
  double reporting).

Driven by the ranks-301-500 audit (ry 0.4.0 on the top-500 CRAN packages).
Minor bump: RY050's dispatch semantics, RY097's collapse criteria, and the
new binding/quoting/narrowing rules intentionally change reported
diagnostics between versions.

### Performance

- Pipe-chain inference was exponential: each `%>%`/`|>` stage re-inferred
  its entire left-hand side inside the desugared call, so a 20-stage chain
  took ~14 s and longer chains never finished. The inferred LHS type is
  now reused. gt (289 R files, previously unscannable) checks in ~2.4 s.
- The required-parameter force-flow analysis walked each `if` branch twice
  (once for "forces", once for "falls through"), which is exponential on
  long `else if` dispatcher chains. Both facts are now computed in one
  pass. lavaan and stargazer (previously >60 min, never completed) check
  in ~2.3 s and ~0.8 s.

### Fixed

- `assign("name", value, envir = ...)`, `makeActiveBinding()`, and
  `delayedAssign()` with a literal name now create package-level bindings
  (any nesting depth under `R/`). Removes whole-package RY010 cascades in
  clock (204 -> 0), rJava, otel, parallelly, and others. `.packageName`
  is bound in every package namespace.
- A string-literal assignment target (`"Math.foo" <- function(...)`) now
  binds, aliases, and establishes S3 dispatch context (`.Generic`,
  `.Method`) exactly like an identifier target (chron 35 -> 10).
- `alist()` arguments are quoted, never resolved as variables, and the
  call returns a list (Deriv 111 -> 8, ade4 RY010 42 -> 2).
- A union whose members are all functions is callable; RY070 no longer
  fires on `f <- if (p) function(...) ... else function(...)` followed by
  `f(...)`. Argument checks report only findings that hold for every
  member signature. NULL/function unions still report RY070.
- RY097 (not-R-source) now also collapses files that mostly parse as R
  but are riddled with parse errors (>= 5 errors and >= 15% of top-level
  statements): Ratfor, GAUSS, and markdown-table files under `inst/`
  (pacman 270 -> 27, plm 136 -> 36 total).
- `is.list()`/`is.function()`/`is.environment()`/`is.data.frame()` guards
  narrow a parameter whose type came only from its default, so
  `f <- function(x = FALSE) if (is.list(x)) x$field` no longer reports
  RY061 (visNetwork 34 -> 5 RY061).
- Assignments nested inside call arguments of `if`/`while` conditions
  (`if (grepl(p, ti <- text[i]))`) now bind in the enclosing scope
  (litedown 27 -> 10).
- RY033's message no longer claims R compares "byte values"; R coerces
  the numeric operand to character and compares lexicographically.
- The typeshed ships registered-but-unexported base S3 methods (e.g.
  `stats:::print.anova`), so RY050 no longer reports them missing
  (spatial, Cairo). RY050 also honors `<generic>.default` as a valid
  dispatch fallback: `coef(glm_fit)` no longer reports a missing
  `coef.glm`. Consequently RY050 can no longer fire for generics that
  have a `.default` method (such as `print`) — dispatch always succeeds
  for them.

### Removed

- `RY095` (negation-comparison-precedence) is retired. The rule assumed C
  operator precedence, but R gives unary `!` lower precedence than
  comparison operators: `!x == y` parses as `!(x == y)`, so every flagged
  site was correct code and the suggested rewrite was a semantic no-op.
  The rule number will not be reused.

### Fixed

- `RY096` no longer fires in functions whose formals include `...`:
  there, `hasArg(name)` legitimately tests for a dots-supplied argument
  (`if (hasArg(b)) list(...)$b`). All 84 corpus hits were this idiom.
  The rule now only flags the provable case — a `hasArg()` naming a
  non-formal in a function without `...` is always `FALSE`.

### Corrections to 0.4.0 release notes

- The scales `!length(x) == 1` guards cited as newly found bugs were not
  bugs; they parse as `length(x) != 1` and behave as intended. The same
  applies to RY095 reports in rpart, mice, quantreg, spdep, and mlflow.

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

[Unreleased]: https://github.com/sims1253/ry/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/sims1253/ry/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/sims1253/ry/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/sims1253/ry/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/sims1253/ry/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sims1253/ry/releases/tag/v0.1.0
