# coffeestats — ry editor-testing example

A tiny, fake R package for manually eyeballing ry diagnostics in VS Code
and Zed. It is not a real package: most of `R/` contains intentional
defects, one per section, each labelled in a `#` comment with the rule
code a reader should see.

**Everything below is verified reality.** The table was produced by
running `ry check .` on this exact tree — not by intent — and every
"silent" claim was run the same way. Line numbers refer to this tree.

## Layout

| path | purpose |
| :--- | :--- |
| `DESCRIPTION` | makes the directory a plausible package root (Imports: dplyr, purrr) |
| `NAMESPACE` | `importFrom(dplyr, mutate, select)` + `importFrom(purrr, map_dbl)`; exercised by `R/daily-report.R` and `R/resolution.R` |
| `ry.toml` | `packages = ["dplyr"]`: attaches dplyr project-wide for the data-mask NSE model |
| `R/prices.R` | clean control; unicode identifiers (`café_latte_price`, `` `☕` ``, `` `📈` ``); defines `TAX_RATE`/`price_with_tax()` read cross-file |
| `R/menu.R` | clean control; cross-file reads; defines `order_total()` called from `R/resolution.R` |
| `R/resolution.R` | project-wide name resolution: cross-file call, NAMESPACE import, unknown callee, non-imported name |
| `R/daily-report.R` | dplyr NSE (`library(dplyr)`, `select`/`mutate`/`summarise`) with one schema-checked typo |
| `R/quality.R` | call-argument diagnostics on base functions + inline-suppression contract |
| `R/warts.R` | one verified type/operator diagnostic per line |
| `R/broken.R` | deliberate syntax-error region |

## Verified diagnostics

From this directory, `ry check .` reports
`checked 7 file(s), 11 error(s), 18 warning(s)` and exits 1.

| file | diagnostics actually reported |
| :--- | :--- |
| `R/prices.R` | none — unicode names must be squiggle-free |
| `R/menu.R` | none — cross-file reads resolve |
| `R/resolution.R` | RY010 @ 33 (`walker <- walk`: purrr name absent from NAMESPACE) |
| `R/daily-report.R` | RY010 @ 43 (`sum(unitss)` typo in a top-level `summarise()` with a known schema) |
| `R/quality.R` | RY091 @ 17 + RY090 @ 17 (`length(xx = 1L)`); RY091 @ 20 (`length()`); RY092 @ 29 (`mean("not numeric")`); RY093 @ 33; RY094 @ 36; RY010 @ 48 and @ 53 (the two unsuppressed twins; @ 53 spans a non-ASCII identifier) |
| `R/warts.R` | RY031 @ 21, 22; RY032 @ 24; RY033 @ 26; RY034 @ 29, 30; RY040 @ 32; RY041 @ 34; RY042 @ 37; RY060 @ 40; RY061 @ 43; RY070 @ 46; RY099 @ 49; RY002 @ 54 |
| `R/broken.R` | RY000 @ 16 (three spans: cols 12, 24, 29), RY000 @ 18; RY010 @ 21 (`daily_shots`) leaked from the recovered region |

Column positions are visible in `--output-format concise` / `json` output.

## Suppression contract

The end of `R/quality.R` writes the same RY010 four times: two lines are
suppressed (`# ry: ignore[RY010]` and the `# noqa: RY010` alias), two are
live — one ASCII, one with `misspelled_variablé`. Only the two live lines
may squiggle. ry also supports a standalone next-line form (`# ry: ignore`
alone on a line) and `# ry: ignore-file`; neither is used here.

## Known non-diagnostics (verified silent — not editor bugs)

Each of these looks diagnosable and produced **no** diagnostic under
`ry check` on this tree:

1. **Call to a nonexistent function.** `daily_revenue(totals)` in
   `R/resolution.R` resolves nowhere; unknown callees stay opaque and
   nothing fires (no RY010, no RY070).
2. **User-defined functions get no argument checking on the project-wide
   path.** `short <- order_total()` in `R/resolution.R` supplies none of
   the required formals and is silent. The single-file checker used by
   the unit-test fixtures does flag this shape (fixture
   `err_user_fn_missing_required.R`), but the project-wide path shared by
   `ry check` and the LSP does not. That is why `R/quality.R` uses base
   functions (`length`, `mean`, `sprintf`) for the argument rules.
3. **Misspelled column on an unknown schema.** A typo inside
   `summarise()` is silent when the data frame is a function *parameter*
   (unknown-schema policy); the identical typo at top level with a known
   schema fires RY010. `R/daily-report.R`'s header documents the
   contrast; only the top-level instance is present in the file.
4. **Bare tidyselect columns in `select()` are not schema-checked.**
   `select(sales, itemm)` in `R/daily-report.R` is silent even though the
   schema of `sales` is known.
5. `list(1) > 2` (comparison against a list) is no-diag by design per the
   upstream fixture `ry030_compare.R`; verified during construction and
   deliberately not included in this tree.

## How to verify quickly

``` bash
# from the ry repo root
cargo build -p ry-cli
cargo run -p ry-cli -- check editors/example-project

# or directly, from this directory
../../target/debug/ry check .
../../target/debug/ry check . --statistics
../../target/debug/ry check . --output-format concise
```

Expected summary: `checked 7 file(s), 11 error(s), 18 warning(s)`,
exit code 1. The same diagnostics are what the LSP should publish: the
CLI and the LSP wrap the same checker core (`ry-checker::Project`), so
any divergence between this table and an editor's squiggles is a
finding, not a rendering quirk (#89).

## What to eyeball in the LSP beyond squiggles

- **Publish on open.** Each opened file should show exactly its row from
  the table — codes and severities (errors and warnings render
  differently in both editors).
- **Publish on edit.** In `R/daily-report.R` fix `unitss` -> `units`: the
  squiggle must clear on the next publish without closing the file;
  undo, and it returns.
- **Cross-file republish.** With `R/menu.R` open, rename `TAX_RATE` to
  `TAX_RATE_OLD` in `R/prices.R` and save. Verified CLI behaviour:
  `menu.R` line 16 gains an RY010 on the `TAX_RATE` default. Check that
  this appears while the menu.R tab is open but not focused, and clears
  when the rename is reverted.
- **Close / reopen (the interesting case).** Close `R/warts.R` (14
  diagnostics) and reopen it: the identical diagnostics must be
  republished with the same spans and severities — neither missing nor
  stale positions from edits made before closing. Also confirm closing
  one file neither clears nor duplicates diagnostics in other open
  files.
- **Recovered-region columns.** In `R/broken.R` the RY000 spans and the
  leaked RY010 must point at plausible columns. Anything after a syntax
  error is documented as unreliable (RY000's own message says so).
- **Non-ASCII positions.** The RY010 on `misspelled_variablé`
  (`R/quality.R` @ 53) must underline the whole identifier: the server
  converts its byte columns to LSP UTF-16 code-unit positions, and an
  off-by-encoding shows up immediately there.
