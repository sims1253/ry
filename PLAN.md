# ry plan (round 3): from correct prototype to teachable tool

## Where we are

Round 2 (the previous PLAN.md) is fully implemented in the working tree:
the oracle suite is green with R installed (44 fixtures), the glue vendor
snapshot exists and is triaged, the LSP is modularized with parse/scope
caching and debounced diagnostics, the CLI has honest flags, and
`cargo test --workspace` + clippy + fmt all pass. Verified 2026-07-04.

The review verdict: the architecture is sound and nothing needs to be
scrapped. Keep tree-sitter -> owned AST -> walk/infer with a fixpoint
over the shared `FnTable`; do NOT adopt salsa in this round. The work
now is of a different kind:

1. The tool's mission is changing. ry is to become a tool for
   **education, practice, and research around principled Bayesian
   workflows in R**. That means the code it must check cleanly is
   tidyverse-flavored workflow code (purrr, dplyr, posterior, brms) and
   parallel execution via purrr's mirai integration (`purrr::in_parallel`,
   purrr >= 1.1.0) -- not just base R. It also means documentation,
   demos, and exercises are first-class deliverables, not afterthoughts.
2. The last systemic false positives (identified by the vendor snapshot's
   own triage) must die before any teaching material ships -- a checker
   that cries wolf teaches the wrong lesson.
3. `crates/ry-checker/src/lib.rs` is 7,168 lines. The LSP got split in
   round 2; the checker did not. This is the one piece of structural rot.

Execute phases in order. Phase 5 (docs/teaching) may proceed in parallel
with 3/4 once Phase 1 and 2 are done.

Build/test gate before every commit: `cargo test --workspace && cargo
clippy --workspace --all-targets -- -D warnings && cargo fmt --all --
--check`. Oracle (requires R): `cargo test -p ry-checker --test oracle
-- --ignored`.

Repo rules: the user runs git themselves -- prepare changes, do not
commit unless asked. No emojis anywhere. Do not delete files without
asking; move superseded material instead.

---

## Phase 0 -- Hygiene (blocking, tiny)

1. **License files.** The workspace claims `MIT OR Apache-2.0` but the
   repo root has no LICENSE file. Add `LICENSE-MIT` and
   `LICENSE-APACHE` (standard texts, copyright "ry contributors").
2. **Crate metadata.** Every `crates/*/Cargo.toml` lacks `description`.
   Add one line each; add `keywords = ["r", "linter", "static-analysis",
   "type-checker"]` and `categories` to `ry-cli`. Point `readme` at the
   workspace README where sensible.
3. **Typeshed audit for fabricated entries.** `base_r.json` contains at
   least two functions that DO NOT EXIST in base R: `identical_to` and
   `colsum` (R has `rowsum` and `colSums`, not `colsum`). Add a check to
   the oracle CI job: a small R script that walks every function name in
   the typeshed and asserts `exists(name)` in a vanilla R session
   (search all default-attached packages). Fix what it finds. This
   prevents hallucinated entries from accumulating.

## Phase 1 -- Kill the remaining systemic false positives

These four items come straight from the vendor snapshot triage in
`crates/ry-checker/tests/vendor_snapshot.rs`. After all four, the glue
snapshot should contain ZERO diagnostics -- update it and assert
emptiness in the triage comment.

### 1.1 RY002 must not fire on Unknown-length conditions (dominant FP)

`walk_stmt`'s `Stmt::If` arm (`crates/ry-checker/src/lib.rs:1302`) and
`infer_if_expr` (~line 2104) emit RY002 whenever the condition's length
is not `One` -- including `Length::Unknown`. Six of the twelve glue
diagnostics are this. `if (!inherits(x, "foo"))` types as
logical<len=?> and warns.

Fix: RY002 fires ONLY on `Length::Known(n) if n > 1` (and `Length::Zero`
stays RY001 via `invalid_condition`). Audit for a third emission site
(`while`) and apply the same rule. Update the RY002 summary in
`rules.rs` to say "condition length is known to be greater than 1".
Corpus fixture: `ok_if_unknown_length_condition.R` with the
`!inherits(...)` and bare-parameter patterns from glue.

### 1.2 Null-narrowing on `is.null` guards

`color.R:123` FP: a parameter defaulting to `NULL`, guarded by
`if (is.null(x)) ... else x(out)`, still types as NULL in the else
branch and fires RY070. The `extract_type_narrowing` machinery exists
for `is.numeric`-style predicates; extend it to `is.null`:
then-branch narrows the binding to NULL, else-branch narrows it AWAY
from NULL. There is no complement type in the model, so else-branch
narrowing replaces a known-NULL (or NULL-containing-union) binding with
`RType::unknown()` (or the union minus the NULL member). Also handle the
negated form `if (!is.null(x))` (swap branches).

### 1.3 Model `.Call` and friends

`glue.R:187/319` FP: `.Call(glue_, ...)` treats the C entry-point symbol
as a variable read and fires RY010. In `infer_call`, when the callee is
one of `.Call`, `.C`, `.Fortran`, `.External`, `.External2`,
`.Internal`: do not treat a bare-identifier FIRST argument as a variable
reference (skip RY010 for it), infer remaining args normally, return
`RType::unknown()`.

### 1.4 Typeshed additions (missing base functions seen in the wild)

Add: `lengths` (integer, length unknown), `delayedAssign` (NULL),
`inherits` (logical, length 1), `requireNamespace` (logical, length 1),
`isTRUE`, `isFALSE`, `nzchar`, `xor`, `Negate` (function), `Recall`
(unknown), `on.exit` (NULL), `match.arg` (character, length 1). All
predicates return length-1 logical -- that plus 1.1 is what silences the
glue RY002s at the root.

Acceptance: glue vendor snapshot is empty; oracle stays green; add a
SECOND vendored package to keep the net honest now that glue is clean
(candidate: a small MIT/GPL-compatible tidyverse-adjacent package with
purrr usage -- check the license, include it, triage its snapshot the
same way).

## Phase 2 -- Package awareness and the workflow typeshed

The single biggest semantic hole: ry has no notion of `library()`. The
dplyr NSE verbs (`filter`, `select`, `mutate`, ...) are recognized by
bare name unconditionally (`NseVerb::from_name`,
`crates/ry-checker/src/lib.rs:144`), which mis-types `stats::filter` in
code that never loads dplyr. Meanwhile the packages the target audience
actually uses (purrr, posterior, brms, mirai) are absent entirely.

### 2.1 Track loaded packages

- `Checker` gains a `loaded: HashSet<String>` populated from
  `library(pkg)` / `require(pkg)` / `requireNamespace("pkg")` calls
  (the library/require special case at `lib.rs:2267` already sees the
  bare name; record it instead of only returning NULL).
- `Project` unions loaded packages across files (R scripts `source()`
  each other; per-file precision is not worth the FPs).
- `ry.toml` gains `packages = ["dplyr", ...]` to declare packages loaded
  implicitly (e.g. via a startup file); merge with the detected set.
- Qualified calls `pkg::fun(...)` resolve against pkg's typeshed without
  requiring `library(pkg)`. Verify the parser/lowering preserves `::`
  (add a test: `dplyr::filter(df, x > 1)` and `stats::filter(x, rep(1, 3))`
  resolve differently).

### 2.2 Split the typeshed by package

- Rename `data/base_r.json` to `data/base.json` (it holds base + stats +
  utils; splitting those three is optional -- they are always attached).
- New files: `data/dplyr.json`, `data/purrr.json`, `data/mirai.json`,
  `data/bayes.json` (posterior, brms, loo, cmdstanr, bayesplot --
  minimal entries, mostly opaque-with-class returns).
- `ry-typeshed` gains `load_package(name) -> Option<&Typeshed>` and a
  merged-view API respecting load order (later `library()` masks
  earlier). `load_base_cached` keeps working for base.
- Gate the dplyr NSE verbs on dplyr (or tidyverse) being loaded or the
  call being `dplyr::`-qualified. Un-gated `filter` resolves to
  `stats::filter` (returns opaque ts). `subset`/`with`/`within`/
  `transform` stay always-on (they are base).

### 2.3 purrr and mirai (the parallel-execution story)

Model the purrr map family exactly like the base higher-order builtins
(extend `HigherOrderFunc` or add a parallel `PurrrFunc` enum -- prefer
extending, the machinery in `ho_lapply`/`callback_return_type` is
reusable):

- `map(x, f)` -> list of f's return (lapply semantics).
- `map_lgl/int/dbl/chr/vec(x, f)` -> vector of the target mode, length
  unknown; check the callback's return mode against the target mode and
  emit RY040-adjacent diagnostics on mismatch (new fixture pair).
- `map2`, `pmap`, `imap` -> list; `walk/walk2` -> invisible first arg.
- `keep`, `discard` -> same type as input; `reduce`, `accumulate` ->
  like `Reduce`.
- `in_parallel(f)` (purrr >= 1.1.0) -> returns `f` unchanged
  (type-transparent wrapper). This is the key entry: workflow code will
  read `map(sims, in_parallel(function(s) fit_one(s)))` and must check
  identically to the sequential version.
- mirai minimal: `daemons(n)` -> invisible, `mirai(expr)` -> opaque
  class "mirai", `collect_mirai`/`call_mirai` -> unknown.

Fixtures: `ok_purrr_map_dbl.R`, `ok_purrr_in_parallel.R`,
`err_purrr_map_dbl_type_mismatch.R`, plus oracle fixtures where R can
arbitrate (purrr installed in the oracle CI job -- add
`r-lib/actions/setup-r-dependencies` or a plain
`Rscript -e 'install.packages(c("purrr","mirai"))'` step).

### 2.4 Bayesian stack minimal signatures

Just enough that the Phase 5 demo checks clean, all in `bayes.json`:
`brms::brm` -> opaque class "brmsfit"; `posterior::as_draws_df` ->
data.frame-classed list with unknown columns; `posterior::summarise_draws`
-> data.frame; `posterior_predict`/`posterior_epred` -> opaque;
`loo::loo` -> opaque class "loo"; `bayesplot::ppc_dens_overlay` ->
opaque class "ggplot". Do not attempt to model draws shapes in this
round.

### 2.5 Typeshed generator (sustainability)

Hand-writing JSON does not scale past base R. Add
`scripts/gen_typeshed.R`: given a package name, emits a DRAFT JSON with
one entry per exported function (`formals()` for params, return type
"unknown"), for a human to refine. Wire the Phase 0.3 `exists()` audit
and this generator into the same script family. The generator is a
curation aid, not an oracle -- say so in its header.

## Phase 3 -- Checker structure and parallel execution

### 3.1 Split `ry-checker/src/lib.rs` (7,168 lines)

Same treatment the LSP got in round 2: mechanical module split, no
behavior change, one commit. Target layout:

- `diagnostics.rs` -- `Severity`, `Diagnostic`, `emit`, severity filter.
- `suppress.rs` -- suppression parsing/filtering (lines ~298-575).
- `scope.rs` -- `Scope`, `FnTable`, `ReturnSlots`, fixpoint plumbing.
- `walk.rs` -- `walk_stmt`, branch-binding merge, narrowing.
- `infer/mod.rs` -- `infer`, `infer_binop`, literals, pipes, if/switch.
- `infer/call.rs` -- `infer_call`, `apply_sig`, structure/trycatch.
- `infer/nse.rs` -- `NseVerb` and the `infer_nse_*` family.
- `infer/higher_order.rs` -- `HigherOrderFunc`, `ho_*`, callbacks.
- `infer/index.rs` -- `infer_index`, `$`/`[[`/`[`, column diagnostics.
- Unit tests move next to their subjects; integration tests untouched.

Acceptance: `lib.rs` under 300 lines (module decls, re-exports,
`Checker` struct + constructors); test counts identical before/after.

### 3.2 Parallelize the Rust side with rayon

`Project::check` pass 3 is embarrassingly parallel: per-file emitters
share the tables via `Arc` already (`project.rs:120-129`). Add `rayon`
as a workspace dependency and `par_iter` the emission loop. Parsing in
the CLI (`run_check_once`) can parallelize with one `RParser` per rayon
thread (`thread_local!` -- tree-sitter parsers are not `Send`). Extend
`perf.rs` with a before/after note; keep the 2s budgets.

### 3.3 Parallelize the oracle with purrr + mirai (dogfooding)

The oracle currently spawns one `Rscript --vanilla` per fixture,
serially (~8s wall for 44 fixtures, and growing with Phase 2's new
fixtures). Replace the per-fixture spawn with a single driver:

- `scripts/oracle_driver.R`: takes the fixture directory, sets up
  `mirai::daemons(parallelism)`, and evaluates every fixture via
  `purrr::map(files, in_parallel(function(f) ...))`, each wrapped in
  `tryCatch(eval(parse(f), envir = new.env()), error = ...)`. Emits one
  JSON object per fixture (`{file, errored, message}`) on stdout.
- Isolation caveat: daemons persist across fixtures, so a fixture that
  attaches a package or writes globals could leak state. Document this
  in the driver header; fixtures must stay side-effect-free (they are
  today). Keep the old per-fixture `Rscript` path in `oracle.rs` as a
  fallback when purrr/mirai are not installed (probe with a quick
  `Rscript -e 'requireNamespace("purrr")'`).
- `oracle.rs` invokes the driver once, parses the JSON, and applies the
  existing must-flag/must-pass/known-gap logic unchanged. The
  stderr-contains-"Error" heuristic dies with the old path (the driver
  reports errors structurally); note that locale-dependent matching was
  a latent bug.
- CI: install purrr + mirai in the oracle job so the parallel path is
  what CI exercises.

This is deliberately the same purrr/mirai pattern the Phase 5 demo
teaches -- the repo uses the tool the docs preach.

## Phase 4 -- CLI and LSP UX

1. **Real color output.** `--color` is parsed but has no effect
   (`color.rs`). Implement it: `anstream` + `owo-colors` (or manual ANSI
   if the dependency footprint offends). Style like ruff/ty: bold file
   path, red "error" / yellow "warning", dimmed rule code, the caret
   line colored to match severity. `auto` = isatty && !NO_COLOR; honor
   CLICOLOR_FORCE. Update the README section that currently documents
   the flag as inert.
2. **`full` becomes the default output format** (ty's default). The
   caret line should underline the whole span (`^~~~~`), not a single
   `^` (`format.rs:91`). `concise` remains available via flag/config.
3. **Human diagnostics go to stdout.** `run_check_once` prints
   concise/full to STDERR (`main.rs:621`), so `ry check > log` captures
   nothing. Flip: diagnostics -> stdout, the summary line and watch-mode
   chrome -> stderr (matches ruff). Machine formats already use stdout.
4. **"Did you mean" suggestions.** Cheap, high teaching value:
   - RY060 (undefined column): suggest the nearest column name
     (Levenshtein distance <= 2) and list available columns (already
     partially done in `emit_undefined_column` -- verify and extend).
   - RY010 (unbound variable): suggest the nearest in-scope binding.
   - RY070: mention where the non-function value was bound ("`f` was
     assigned `integer` at line 3") if the binding site is known.
   Render as a `help:` second line in `full` format and as
   `relatedInformation` in the LSP.
5. **`ry check --statistics`**: per-rule counts after the run (ruff's
   `--statistics`). Ten lines of code, essential for the Phase 6
   corpus-research use case.
6. **Watch mode**: replace the 500ms poll with the `notify` crate;
   print the check duration and a timestamp on each cycle.
7. **`ry rule` alias** for `ry explain-rule` (matches `ruff rule`), and
   `ry explain-rule --output-format markdown` emitting the long-form
   rule doc (see Phase 5.2) so docs and CLI share one source.
8. **R Markdown / Quarto support.** The education audience writes
   `.qmd`/`.Rmd`, not bare `.R`. Teach the CLI to extract fenced
   ```` ```{r} ```` chunks (concatenated per document, chunk options
   ignored except `eval=FALSE` which skips the chunk), map spans back to
   host-file line numbers, and check them as one virtual file.
   `collect_r_files` picks up `.qmd`/`.Rmd`/`.rmd`. This is the largest
   Phase 4 item; land it last and behind solid fixtures (a demo .qmd
   with a seeded error on a known line asserting the reported line
   number is the HOST file's).
9. **LSP workspace scan.** `State.root` is stored and unused
   (`backend.rs:68`). At `initialize`, scan the workspace for `.R` files
   and include them in the `Project` so cross-file resolution works for
   files that are not open. Cache by mtime; this is not salsa, just a
   coarse preload.

## Phase 5 -- Documentation, demos, exercises, teaching material

This phase is a deliverable, not decoration. The audience: R users in
applied statistics who have never run a static analyzer, students in a
Bayesian workflow course, and researchers who want to instrument code
quality.

### 5.1 Docs site as Quarto

`docs/` is a Quarto website (the audience knows Quarto; it renders R
chunks and can literally run `ry` in bash chunks during CI). Pages:

- `index.qmd` -- what/why, 90-second tour with real output.
- `getting-started.qmd` -- install, first check, reading a diagnostic.
- `rules/index.qmd` + one page per rule (see 5.2).
- `types.qmd` -- the type model: Mode/Length lattice, unions at
  control-flow merges, what "opaque" means, and a table of "what R does
  at runtime vs what ry says" for each teaching example. This page IS
  the education artifact -- it teaches R's coercion semantics through
  the checker's eyes.
- `configuration.qmd` -- ry.toml, suppression, severity model.
- `editors.qmd` -- VS Code, Neovim, Helix, and Positron setup (Positron
  matters for this audience).
- `ci.qmd` -- GitHub Actions recipes using `--output-format github`.
- `architecture.qmd` -- crate map, three-pass Project check, fixpoint,
  oracle methodology. Doubles as the research-methods description.
- `faq.qmd` -- vs lintr, vs R CMD check, why Rust, soundness stance
  (ry prefers silence over false positives; what that trades away).

CI: a `docs` job renders the site (needs R + Quarto + the built `ry`
binary on PATH so bash chunks run for real -- output in docs can never
drift from behavior).

### 5.2 Rule docs from one source of truth

Extend `rules::Rule` with `explanation: &'static str` (long-form
markdown: what R does at runtime, a bad example, a good example, when
to suppress) and `example: &'static str` (a runnable snippet). The
Quarto rule pages are generated from this table
(`scripts/gen_rule_docs.rs` or a small xtask); `ry explain-rule RY040`
prints the same text. A unit test asserts every rule has a non-empty
explanation and that the bad example actually triggers the rule (run
the checker on it in the test) -- self-verifying documentation.

### 5.3 The Bayesian workflow demo (flagship)

`demos/bayesian-workflow/` -- a Quarto document implementing a
principled workflow on a simulated dataset, structured by stages:
prior predictive checks, fitting (brms; chunks tagged `eval=FALSE` so
docs CI does not need Stan -- ry checks code without running it, which
is exactly the point), convergence diagnostics via posterior,
posterior predictive checks, and simulation-based calibration where the
replications run through `purrr::map(sims, in_parallel(...))` with
`mirai::daemons()`. The demo must check completely clean with Phase 2's
typeshed. A closing section shows `ry check demos/bayesian-workflow`
output as proof.

`demos/bayesian-workflow-broken/` -- the same workflow with ~10 seeded,
realistic bugs (a misspelled column after a `mutate`, a
character-vs-numeric comparison on a factor level, `lapply` where the
callback receives the wrong shape, an unbound variable inside a purrr
lambda, calling a data frame as a function, a length->1 vector in an
`if`). Each bug is one ry diagnostic. `ANSWERS.md` maps each diagnostic
to the workflow mistake it represents and WHY the statistical result
would have been silently wrong -- this file is the teaching payload.
A CI test pins the expected diagnostic set (insta snapshot) so the
exercises never rot.

### 5.4 Exercises (koans)

`exercises/01_conditions.R` through `~08_higher_order.R`: each file
opens with instructions in comments and contains code that fails
`ry check`; the student edits until green. Ordered to teach the type
model incrementally (conditions -> arithmetic/coercion -> scoping ->
data frame schemas -> NSE -> closures -> higher-order -> purrr).
`exercises/README.md` explains the loop (`ry check exercises/01_*.R`,
fix, repeat). Solutions live in `exercises/solutions/` with a CI test
asserting every solution is clean and every exercise is NOT.

### 5.5 Repo-level docs

- `README.Rmd` renders to `README.md` (the standard R-community
  convention; being rendered with knitr, its output blocks can never
  lie). The README rewrite is handled separately from this plan -- do
  not edit `README.Rmd`/`README.md` beyond keeping the render fresh.
- `ARCHITECTURE.md` -- one page: crate map, data flow, where to add a
  rule (link to the rules-from-one-source machinery), how the oracle
  and vendor nets work.
- `CONTRIBUTING.md` -- build/test gate, fixture conventions
  (`ok_*`/`err_*`/oracle tags), the no-false-positives bar for new
  rules (every new rule needs an oracle or vendor justification).
- Logo: deferred. Leave `assets/logo-concept-*.svg` untouched; do not
  wire any logo into the README or docs site this round.

## Phase 6 -- Backlog (documented, NOT implemented this round)

Keep this list at the bottom of PLAN.md when revising; these are the
research/future directions the docs may reference as "planned":

- **WASM playground**: ry-core/ry-checker compile to wasm32 (tree-sitter
  supports it); a browser playground like ruff's is the single biggest
  education multiplier. Blocked on nothing technical, just effort.
- **Salsa/incrementality** and LSP workspace indexing beyond the Phase
  4.9 preload.
- **Possibly-unbound diagnostics** (RY011): branch-merge machinery
  already distinguishes one-branch bindings; needs a low-FP design.
- **NA tracking**: the `na` flag exists in the typeshed JSON but is
  unused by the checker.
- **S4 / R6 / environments; NAMESPACE parsing** for package development.
- **Distribution**: cargo-dist for prebuilt binaries on GitHub
  releases; an R wrapper package (install ry via
  `install.packages`-adjacent UX, like `styler`/`air` precedents);
  crates.io publish once the name is settled.
- **Research instrumentation**: `scripts/cran_survey.R` -- run ry over
  the top-N CRAN packages, aggregate `--output-format json` +
  `--statistics`, measure diagnostic rates per rule; the methodology
  section in `architecture.qmd` describes how to cite it. Study idea:
  does ry-in-the-editor change error rates in student Bayesian
  workflow assignments?
- **Property-based oracle**: generate random well-typed/ill-typed R
  programs and compare ry vs R, instead of hand-written fixtures only.

## Explicitly out of scope this round

Salsa; new diagnostic codes beyond what Phases 1-2 need; S4/R6;
modeling draws shapes/dimensions in the Bayesian typeshed; the WASM
playground; auto-fix.

## Suggested commit sequence

Phase 0 as one commit. 1.1-1.4 as four commits (fix + fixtures each),
then one commit re-snapshotting glue + adding the second vendor package.
2.1, 2.2, 2.3, 2.4+2.5 as four commits. 3.1 as ONE mechanical commit,
3.2 and 3.3 separately. Phase 4 items individually (4.8 last). Phase 5:
5.1+5.2 together, 5.3, 5.4, 5.5 separately. Run the full gate before
each; run the oracle whenever fixtures or the typeshed change.

## Appendix: regression probes for Phase 1 (all must be clean)

```r
# 1.1 -- unknown-length conditions stay quiet (no RY002):
f <- function(x) { if (!inherits(x, "foo")) stop("nope"); x }
g <- function(flag) { if (flag) 1 else 2 }

# 1.2 -- null-guard narrowing (no RY070 in the else branch):
h <- function(fun = NULL) {
  if (is.null(fun)) identity(1) else fun(1)
}

# 1.3 -- .Call first arg is not a variable read (no RY010):
glue_c <- function(x) .Call(glue_, x)

# 1.4 -- predicates are length-1 logical (no RY001/RY002):
if (requireNamespace("purrr", quietly = TRUE) && isTRUE(TRUE)) print(1)

# Phase 2 -- package gating (stats::filter is NOT dplyr):
y <- stats::filter(1:10, rep(1/3, 3))   # no NSE, no RY060
library(dplyr)
d <- filter(data.frame(a = 1), a > 0)   # NSE resolves column `a`

# Phase 2.3 -- purrr + mirai (all clean):
library(purrr)
library(mirai)
daemons(2)
out <- map_dbl(1:4, in_parallel(function(i) i * 2))
```
