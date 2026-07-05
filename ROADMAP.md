# Roadmap

Where ry is headed, roughly ordered. The mission: a fast, honest
checker for R that supports education, practice, and research around
principled Bayesian workflows. Nothing here is a promise; everything
here has been thought through enough to start.

## Near term (correctness and polish)

- **Finish the checker module split.** `ry-checker/src/lib.rs` is still
  ~7.5k lines; diagnostics and suppression are extracted, the
  walk/infer/NSE/higher-order/index families are not.
- **Colored output.** `--color` is parsed and validated but the human
  formats render plain. Implement auto/always/never with NO_COLOR and
  CLICOLOR_FORCE support, styled like ruff/ty.
- **"Did you mean" suggestions.** RY060 already lists available
  columns; add nearest-name suggestions (edit distance) there, for
  RY010 (unbound variable) against in-scope bindings, and a
  binding-site note for RY070.
- **Event-driven watch mode.** Replace the 500 ms poll with the
  `notify` crate; print check duration per cycle.
- **R Markdown / Quarto support.** Extract `{r}` chunks from
  `.Rmd`/`.qmd`, check them as one virtual file per document, map spans
  back to host-file line numbers. The target audience writes Quarto,
  not bare `.R`.
- **LSP workspace preload.** Scan the workspace root at `initialize` so
  cross-file resolution covers files that are not open in the editor.
- **rlang / vctrs stubs.** The purrr vendor snapshot's remaining
  diagnostics are all cross-package names from rlang and vctrs;
  covering the common subset empties that snapshot too.

## Teaching materials

- **Quarto docs site** (`docs/`): getting started, the type model as a
  guided tour of R's runtime semantics, per-rule reference pages
  generated from the rule registry (single source of truth,
  self-verifying examples), editor setup including Positron, CI
  recipes, architecture.
- **Bayesian workflow demo**: a principled workflow (prior predictive
  checks, fitting, convergence diagnostics, posterior predictive
  checks, parallel simulation-based calibration via
  `purrr::in_parallel()` on mirai daemons) that checks clean -- plus a
  broken twin with seeded, realistic bugs, one diagnostic each, and an
  answer key explaining the statistical consequence of every bug.
- **Exercises**: koan-style files edited until `ry check` is green,
  ordered to teach the type model incrementally; solutions verified in
  CI.

## Longer term

- **WASM playground.** ry-core/ry-checker compile to wasm32; a browser
  playground is the single biggest education multiplier.
- **Incrementality (salsa)** and real workspace indexing in the LSP.
- **NA tracking.** The `na` flag exists in the typeshed JSON but is
  unused by inference.
- **Possibly-unbound diagnostics.** The branch-merge machinery already
  distinguishes one-branch bindings; needs a design with a defensible
  false-positive rate.
- **S4 / R6 / environments; NAMESPACE resolution** for package
  development workflows.
- **Distribution.** Prebuilt binaries (cargo-dist), an R-side
  installer/wrapper package, crates.io release.
- **Research instrumentation.** A CRAN survey script aggregating
  `--output-format json` + `--statistics` across top packages;
  classroom studies on whether in-editor checking changes error rates
  in Bayesian workflow assignments.
- **Property-based oracle.** Generate random well/ill-typed R programs
  and compare ry against R, beyond the hand-written fixture corpus.
