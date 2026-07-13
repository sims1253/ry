# Plan 12: Confidence tiers, shadowed-symbol boost, and baseline files

## Status: implemented (2026-07-12).

Derived from ROADMAP items 15, 16 (baseline half) and amendment A. The audit
shows RY002/RY030/RY033/RY050 were high-precision while RY010 drowned
everything; and the single most productive real-bug pattern across 300
packages is a typo'd local that silently resolves to a function from
base/stats/graphics (`col` -> `base::col`, `open` -> `base::open`, ...).
Users must see the good diagnostics first, and real projects need a way to
adopt ry incrementally.

## Scope — files this plan may touch

- `crates/ry-checker/src/diagnostics.rs`
- `crates/ry-checker/src/lib.rs`
- `crates/ry-checker/src/infer/` (emit sites, minimal edits)
- `crates/ry-checker/src/tests.rs`
- `crates/ry-cli/src/main.rs`
- `crates/ry-cli/src/config.rs`

## Part A — `Confidence` on `Diagnostic`

- Add `pub enum Confidence { Low, Medium, High }` (ordered, serializable to
  lowercase strings) and a `confidence` field on `Diagnostic`.
- Default assignment BY RULE CODE at construction (a helper
  `Confidence::default_for(code)`), so emit sites need no changes:
  - High: RY030, RY033, RY050, RY070, RY092, RY093, RY094, RY095, RY096
    (resolver-certain / structurally-exact rules).
  - Medium: everything else, including RY010.
  - Low: RY097 (info-class), anything already info severity.
- Path-based demotion happens in the CLI (Part D), not in the checker.

## Part B — shadowed-symbol confidence boost (amendment A)

When a symbol is used in VALUE position (not call position) and it resolves
ONLY to a function from an attached namespace / the ambient function set
(`globals.ambient_functions` — see how `infer/mod.rs` consults the split,
and `has_function_anywhere` in `lib.rs`), that use is near-certainly a typo
for a missing local (`col`, `labels`, `segments`, `profile`, `open`, ...).

- Find the emit sites where this situation is visible today (RY030
  closure-in-comparison and friends; possibly the RY010 path when the name
  is unbound locally but present as an ambient function).
- Set `confidence: High` on those diagnostics and extend the message with
  the resolution target, e.g. "`col` here resolves to the base function
  `col()`; possible missing local variable".
- Do NOT create a new rule code; this is a tiering + message improvement on
  existing diagnostics. Add focused tests for the quantmod/xts/cli shapes:
  `x <- col` and `if (identical(oldClass, "zoo"))` style value uses.

## Part C — baseline files

- `ry check --write-baseline <path>`: after a normal run, write a JSON file
  `{"version": 1, "entries": [{"path": "...", "code": "RY010",
  "message": "...", "count": 2}, ...]}` where `path` is repo-relative,
  entries are unique (path, code, message) with occurrence counts, sorted
  stably. NO line numbers — baselines must survive unrelated edits.
- `--baseline <path>` (CLI flag + `baseline` config key): load the file and
  subtract matching diagnostics — each (path, code, message) entry absorbs
  up to `count` matching diagnostics; excess ones still surface. Apply
  after comment-suppression filtering in `run_check_once`.
- A missing baseline file passed via flag is a hard error; via config key,
  a warning.
- Exit-code semantics: baselined-away diagnostics do not affect exit code.

## Part D — output ranking and `--min-confidence`

- Sort diagnostics: confidence High -> Low, then file, then line (find the
  existing `sort_and_deduplicate_diagnostics` or equivalent in the CLI).
- CLI-side demotion by one tier (High->Medium->Low, floor Low) when the
  file lives under `tests/`, `data-raw/`, `demo/`, `vignettes/`, `inst/` of
  a package root.
- `--min-confidence <low|medium|high>` (default low = no filtering) filters
  the output AND the exit-code computation.
- Human output: show the tier only when not Medium, e.g. a `[high]` /
  `[low]` tag; JSON output (if there is one — check main.rs) always carries
  the field.
- Update `ry check --help` text accordingly.

## Tests / acceptance

- Checker tests: default confidence per code; shadowed-symbol boost message
  and tier.
- CLI tests: write-baseline then re-check with `--baseline` yields zero
  diagnostics and exit 0; a NEW diagnostic (not in baseline) still
  surfaces; count semantics (2 baselined, 3 present -> 1 surfaces);
  `--min-confidence high` hides Medium; tests/-path demotion.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted changes.
