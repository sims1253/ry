# Plan 21: Tidyverse NSE completion — derive eval metadata from Rd docs

## Status: implemented (2026-07-13).

The remaining tidyverse corpus noise (dplyr 279, dtplyr 274, dbplyr 269,
tidyr 216, tidyselect 127 — almost all RY010 on column symbols in
tests/testthat/) has four verified mechanisms. Corpus packages live
read-only at /home/m0hawk/Documents/ry-audits/packages; probe with
target/debug builds; never rebuild target/release; never modify ry-audits.

Verified stub-coverage gaps: `stubs/dplyr/dplyr.json` has only 28
functions (`add_count`, `do`, `tally`, ... missing), `stubs/tidyr` 17
(`drop_na`, `fill`, `replace_na`, ... missing), and there is NO
tidyselect stub (`select_loc`, `eval_select`, ... used heavily in its own
tests).

## Part A — generate eval metadata from installed-package Rd docs

dplyr/tidyr/tidyselect document NSE per argument in their Rd sources with
literal `<data-masking>` and `<tidy-select>` markers (verified:
`tools::Rd_db("dplyr")[["mutate.Rd"]]` contains "data-masking"; all of
dplyr, tidyr, tidyselect, dbplyr, dtplyr are installed locally).

Write `/home/m0hawk/Documents/r-typeshed/scripts/gen_nse_metadata.R`
(R only — this repo has no Python) that, for a given package:

1. Reads `tools::Rd_db(pkg)`, maps each Rd file to its exported
   function(s) (via `\alias`es intersected with `getNamespaceExports`).
2. For each documented ARGUMENT whose description contains
   `data-masking` -> eval mode `data_mask`; `tidy-select` ->
   `tidy_select`. Match the argument names against `formals()` of the
   function; `...` is a valid key.
3. Merges into `stubs/<pkg>/<pkg>.json`: create missing function entries
   (params from `formals()`, opaque return following the existing entry
   style) and add/extend their `eval` maps. NEVER remove or overwrite
   existing hand-written metadata (union; existing keys win).
   Keep function keys sorted (the validator warns otherwise).
4. Run it for dplyr, tidyr, and tidyselect (new stub dir; look at an
   existing stub for the schema envelope fields). Do NOT run it for
   dbplyr/dtplyr (their verbs are S3 methods; inheritance covers them).
5. Mirror changed stub files into `crates/ry-typeshed/vendor/` by hand.

Sanity-check the generated metadata (spot-check that e.g. `select`'s
`...` is tidy_select, `mutate`'s is data_mask) and validate with
`cargo run -p ry-cli -- typeshed validate ../r-typeshed/stubs`.

## Part B — `.` placeholder inside data-masked arguments

Verified: dplyr/tests/testthat/test-deprec-do.R:11 `df |> do(head(., 1))`
fires RY010 on `.`. Inside a data-masked argument, bind `.` as opaque
(dplyr's `do`, magrittr idioms). Also confirm the NATIVE pipe (`|>`)
path applies the same NSE gating as `%>%` — the failing site uses `|>`;
if the pipe desugaring differs, fix it there
(`crates/ry-checker/src/infer/pipe.rs`).

## Part C — defused parameters via `{{ }}` embrace

Verified: tidyselect/tests/testthat/test-eval-walk.R:9 —

```r
wrapper <- function(x, var1, var2) select_loc(x, c(-{{ var1 }}, ...))
wrapper(letters2, a, c)   # RY010 on `a`
```

Plan 19 Part C marks parameters defused when first used in `enquo(...)`
etc. Extend the same marking to parameters whose use in the body is
inside a `{{ }}` embrace: such parameters forward unevaluated
expressions, so call-site arguments must not be lexically checked.
(The embrace recognizer from plan 11 already exists — reuse its
structural detection.)

## Part D — probe-driven residual sweep

After A-C, re-probe all five packages
(`target/debug/ry check --statistics --exit-zero <pkg>`). For the
biggest remaining cluster in dtplyr/dbplyr (e.g.
`dt %>% complete(x, y)` at dtplyr/tests/testthat/test-complete.R:9,
`add_count(x, wt = y)` in test-count.R), reduce a failing site to a
standalone repro, identify the mechanism (missing stub entry now fixed
by Part A? pipe path? method-inheritance gap?) and fix it. Report
mechanism + fix. Do not chase clusters under ~15 diagnostics.

## Acceptance

- Per-package before/after numbers for dplyr, dtplyr, dbplyr, tidyr,
  tidyselect (before: 279/274/269/216/127).
- `Rscript --vanilla scripts/gen_nse_metadata.R --check`-style staleness
  mode is NOT required; a plain runnable script is fine.
- Unit tests: `.` inside `do()` silent; embrace-defused wrapper call
  silent (the tidyselect shape above, minus tidyselect-specific calls if
  no stub — use a dplyr verb for the fixture).
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` green; typeshed validate green.
- Preserve all pre-existing uncommitted changes in both repos; no git
  state-changing commands.
