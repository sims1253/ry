# Plan 16: r-typeshed — NSE registry additions, dataset generation, R port of the globals generator

## Status: implemented (2026-07-12). Repo: ../r-typeshed.

All edits in this plan happen in the r-typeshed checkout. Read
`schema/SCHEMA.md` first — every stub you write must conform to it, and the
existing stubs (`stubs/dplyr/dplyr.json`, `stubs/recipes/`, `stubs/dbplyr/`)
are the style reference for `eval` / mask metadata.

Derived from the top-300 CRAN audit (ry-audits ROADMAP items 4 and F):
the listed packages produced large RY010 false-positive clusters because
their NSE / injected-binding semantics are not in the typeshed.

## Part A — port `scripts/gen_standard_globals.py` to R

This repo's convention is R-only tooling (see plan 08 "depython"); the
untracked `gen_standard_globals.py` violates it.

- Write `scripts/gen_standard_globals.R` (Rscript, `#!/usr/bin/env Rscript`,
  runnable via `Rscript --vanilla`) with the SAME behavior and CLI as the
  Python script: default mode updates `stubs/base/base.json`
  (`globals.ambient` + `globals.ambient_functions` split, preserving the
  existing JSON formatting style of that file), and a `--check` mode that
  exits non-zero if the file is stale.
- The regenerated `base.json` must be byte-identical to what the Python
  script produces (run both and diff to prove it; report the diff result).
- Leave the `.py` file in place — do not delete it (the user decides that).

## Part B — mechanical dataset inventory

Extend the R generator (or add a sibling section in it) to also emit the
`datasets` entries mechanically via
`data(package = c("datasets", ...))$results`, instead of hand-curation, for
the base/recommended set already covered by the ambient generator. Keep any
existing hand-written dataset entries that the mechanical pass does not
produce (union, never delete).

## Part C — new stub packages with NSE / injected-binding metadata

Add minimal stubs for the following, modeling ONLY the functions named plus
whatever `eval` metadata kills the audit FP cluster. Follow SCHEMA.md; when
a semantics cannot be expressed in the schema, model the function as opaque
with quoted/unevaluated args and note the limitation in your final message.

1. `patrick` — `with_parameters_test_that(desc_stub, code, ..., .cases)`:
   `code` is evaluated with the case columns injected as bindings; at
   minimum mark `code` as data-masked / quoted so bare case variables stop
   firing RY010 (bit64 cluster, 353 diags).
2. `rex` — `rex(...)`, `rex_mode()`: the `...` is a DSL of unevaluated mode
   tokens (rex cluster, 133 diags). Mark `...` quoted.
3. `rlist` — `list.map`, `list.filter`, `list.select`, `list.sort`,
   `list.group`, `list.update`: second argument is an expression evaluated
   with list-element fields in scope (213 diags). Mark it data-masked.
4. `box` — `use(...)`: import DSL, fully quoted (covr cluster).
5. `zeallot` — `%<-%` and `%->%`: LHS (resp. RHS) is a quoted destructuring
   pattern. Model as far as the schema allows; if operator stubs cannot
   express "LHS introduces bindings", mark args quoted and note that the
   checker needs follow-up support.
6. `future` — `%<-%` (future assignment): same treatment as zeallot.
7. `bench` — `press(...)`: named `...` arguments become bindings visible in
   the final expression argument; `mark(...)` expressions also reference
   them. Mark appropriately.

## Part D — hygiene

- If `scripts/gen_typeshed.R` or SCHEMA.md need small doc updates to mention
  the new generator/datasets flow, make them.
- Do not touch the `ry` repo; do not run any git state-changing commands.
  There are pre-existing uncommitted changes in this repo — preserve them.

## Acceptance

- `Rscript --vanilla scripts/gen_standard_globals.R --check` passes.
- All new/changed JSON parses (`Rscript -e 'jsonlite::fromJSON(...)'` or
  equivalent) and follows SCHEMA.md.
- Final message lists: per-package what was modeled, what could not be
  expressed in the schema, and the Python-vs-R generator diff result.
