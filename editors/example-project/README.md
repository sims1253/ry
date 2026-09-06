# coffeestats: editor playground

Open this folder in VS Code, Positron, or Zed to try ry's diagnostics,
inferred type hints, and suppression actions. The seven R files contain
intentional errors. Check them with ry; do not source or install this example
as an R package.

## Use the current checker

From the repository root, build and check the playground:

```sh
cargo build --locked -p ry-cli
cargo run --locked -p ry-cli -- check editors/example-project
```

The check exits with status 1 and reports seven files, 11 errors, and
18 warnings. Use the binary you just built in the editor too; an older
bundled or PATH binary may produce different findings.

Open `editors/example-project` as the editor workspace. Opening the repository
root also includes the checker's other intentionally failing R fixtures.
The example's `ry.toml` attaches dplyr, and its `NAMESPACE` declares dplyr
and purrr imports. You do not need to install or run those R packages to
check these files.

### VS Code / Positron

Install the ry extension, then set this in your workspace settings using
your actual absolute path:

```json
{
  "ry.path": ["/absolute/path/to/ry/target/debug/ry"]
}
```

On Windows use the path to `ry.exe`, with escaped backslashes or forward
slashes. Trust your own checkout so the extension can use this explicit
binary; untrusted workspaces use the bundled binary. Leave confidence,
rule filters, baseline, and configuration overrides at their defaults for
comparison with the table below. Run `ry: Debug Information` to confirm the
binary path and version. See the [extension guide](../code/README.md) for
settings and restart commands.

### Zed

Install the R and ry extensions. Select ry for R and point it at the same
local binary:

```json
{
  "languages": {
    "R": { "language_servers": ["ry"] }
  },
  "lsp": {
    "ry": {
      "binary": {
        "path": "/absolute/path/to/ry/target/debug/ry",
        "arguments": ["server"]
      }
    }
  }
}
```

Use `ry.exe` on Windows. This explicit path also works when the latest
published release lacks the executable checksum sidecars required by the
extension's automatic downloader.

## Files and expected diagnostics

`prices.R` and `menu.R` are clean controls with Unicode names, ordinary
arithmetic, and references between files. `resolution.R` exercises package
imports and unresolved names. `daily-report.R` contains dplyr data masking.
`quality.R` covers argument checks and suppressions, `warts.R` covers operator
and type errors, and `broken.R` exercises recovery after a syntax error.

The table records code, line, and column from the CLI's JSON output using
the bundled stubs, with installed-library discovery disabled. Positions are 1-based.
The CLI integration test checks this table against the current source files:

```sh
cargo test -p ry-cli --test editor_playground
```

<!-- playground-diagnostics:start -->
| File | Errors | Warnings | Code at line:column |
| :--- | ---: | ---: | :--- |
| R/broken.R | 4 | 1 | RY000@16:12; RY000@16:24; RY000@16:29; RY000@18:1; RY010@21:11 |
| R/daily-report.R | 0 | 1 | RY010@43:51 |
| R/menu.R | 0 | 0 | none |
| R/prices.R | 0 | 0 | none |
| R/quality.R | 1 | 7 | RY091@17:12; RY090@17:19; RY091@20:14; RY092@29:19; RY093@33:20; RY094@36:20; RY010@48:17; RY010@53:25 |
| R/resolution.R | 0 | 1 | RY010@33:11 |
| R/warts.R | 6 | 8 | RY031@21:20; RY031@22:19; RY032@24:23; RY033@26:21; RY034@29:10; RY034@30:10; RY040@32:14; RY041@34:16; RY042@37:15; RY060@40:16; RY061@43:18; RY070@46:18; RY099@49:19; RY002@54:7 |
<!-- playground-diagnostics:end -->

To inspect messages and positions from this folder:

```sh
../../target/debug/ry check . --output-format concise
../../target/debug/ry check . --statistics
```

## Editor checks

Use default checker settings and saved files when comparing CLI and editor
results. The language server analyzes the project, including unopened files;
type hints and suppression actions apply to open documents. ry does not
provide completion, hover, navigation, or rename.

- Open `R/warts.R`. Compare its diagnostics with the table, then close and
  reopen it. The same findings should return without duplicated diagnostics
  in other files.
- In `R/daily-report.R`, change `sum(unitss)` to `sum(units)`. Its RY010
  should clear after the next analysis. Undo the edit and check that it returns.
- Keep `R/menu.R` open. In `R/prices.R`, rename the `TAX_RATE` binding to
  `TAX_RATE_OLD` and save. The default argument in `order_total()` should gain
  RY010 while `menu.R` is still open. Restore the binding and check that it clears.
- In `R/quality.R`, compare the two suppressed `misspelled_variable` lines
  with their live twins. Only the live lines should report RY010. Apply the
  suppression quick fix to a live line; confirm it clears, then undo the edit.
- On `misspelled_variablé`, check that the underline covers the full identifier.
  In `R/prices.R`, the accented and emoji bindings should remain free of errors.
- Enable inlay hints in your editor and inspect the simple assignments in
  `R/prices.R`. Compare inferred types with
  `../../target/debug/ry dump-types R/prices.R`; an unknown type is an analysis limit.
- In `R/broken.R`, inspect the RY000 spans. Findings after the syntax error
  come from a recovered parse tree and can be unreliable.

The table is the CLI expectation, not a record of a manual editor session.
If editor results differ, first check the binary path, settings, and unsaved
contents; record any remaining difference with its rule code and location.

## Cases that currently stay silent

Silence does not prove that R will accept a call. These examples deliberately
record current limits:

- `daily_revenue(totals)` in `resolution.R` has an unknown callee and stays opaque.
- `order_total()` in the same file supplies neither required argument and
  still receives no diagnostic in this package check.
- `select(sales, itemm)` in `daily-report.R` stays silent. Bare tidyselect
  picks are not checked like the known-schema `summarise()` typo above it.
- A data frame parameter has an unknown schema, so names inside
  `units_summary()` are treated as possible columns. A misspelling there can
  stay silent even when the same name is caught against the top-level `sales`.
