# Usage

[Getting started](../README.md) · [Configuration](configuration.md) · [Rules](rules.md)

- [Checking files and CI](#checking-files-and-ci)
- [Package awareness](#package-awareness)
- [Data masking and NSE](#data-masking-and-nse)
- [Dumping inferred types](#dumping-inferred-types)
- [Exporting analysis facts](#exporting-analysis-facts)
- [Editors](#editors)
- [Known limits](#known-limits)
- [Building from source](#building-from-source)

## Checking files and CI

Run `ry check` to check the current directory. Supply files or directories to
narrow the check. ry collects `.R` and `.r` files recursively in each directory.

```sh
ry check .
ry check R/analysis.R R/helpers.R
ry check --watch .
```

The exit status is nonzero when an error remains after filtering.
`--error-on-warning` also fails on warnings; `--exit-zero` lets the check
succeed despite findings. Usage and I/O failures still return a nonzero status.
Human-readable diagnostics use ANSI color on terminals; select the
color policy with `--color auto|always|never`. Automatic color respects
`NO_COLOR`, and machine-readable formats never contain ANSI escapes.

### CI

`--output-format github` emits GitHub Actions annotations. Use `gitlab` for
GitLab Code Quality reports or `junit` for JUnit XML reports. `--statistics` prints
per-rule counts after a run. Once ry is installed, a GitHub Actions step
can run:

``` yaml
- run: ry check --output-format github .
```

## Package awareness

ry tracks `library()` and `require()` calls to resolve function names.
For example, `filter()` means `stats::filter` until dplyr is loaded.
`dplyr::filter(df, x > 0)` resolves the column `x` against `df`'s schema
whether or not dplyr is attached. `requireNamespace()` does not make
unqualified names available.

When checking an R package, ry reads its `NAMESPACE` imports.
`importFrom(pkg, name)` records which package supplies each name. If ry has
no stub for that dependency, it treats the imported value as opaque: the
name is known, but its type is not. Whole-package imports, `library()`, and
`require()` also use installed packages' static `NAMESPACE` exports.
ry does not execute R or load package code.

ry bundles stubs from [r-typeshed](https://github.com/sims1253/r-typeshed)
for base R, tidyverse packages, Bayesian tools, testing frameworks, and other
packages. See the [stub directory](https://github.com/sims1253/r-typeshed/tree/master/stubs)
for current coverage, or run `ry explain typeshed` to list the packages in your
installed version. Declare packages attached outside the checked sources in
`ry.toml`.

When checking a package source tree, ry uses the package's namespace.
Files under `tests/`, `inst/`, `demo/`, and `vignettes/` also see DESCRIPTION
`Depends` / `Suggests` and testthat. Only files under `tests/testthat/`
inherit bindings and attached packages from testthat `helper*` / `setup*`
files. For tinytest, declare the dependency in DESCRIPTION or attach it
in the checked source; ry does not load a tinytest helper context.

`revdep/`, `src/`, snapshot data, and
`.Rbuildignore` matches (never `R/` or `tests/`) are skipped. R files nested
under `tests/` are treated as fixture data unless they are runners at
`tests/` root or `test*`, `helper*`, `setup*`, or `teardown*` files directly
under `tests/testthat/`; set `check-test-fixtures = true` to check fixture data.

Typed purrr maps check the callback's return type. For example, save this as
`parallel.R` and run `ry check parallel.R`:

```r
library(purrr)
bad <- map_dbl(1:4, function(i) as.character(i))
```

ry reports RY080 because the callback returns character values where `map_dbl`
requires doubles. `in_parallel()` preserves the callback's inferred type.

## Data masking and NSE

Non-standard evaluation (NSE) lets an R function interpret an argument as
an expression. Data masking uses this to make data frame columns available
as bare names, such as `mpg` inside `summarise()`.

Stubs declare which parameters are data-masked, tidy-selected, or
quoted (for tidyverse packages this metadata is generated from the
`<data-masking>` / `<tidy-select>` markers in their documentation).
Columns inside a masked argument resolve against the data frame's
schema instead of the lexical scope.

Save this as `nse.R` and run `ry check nse.R`:

``` r
library(dplyr)
d <- data.frame(mpg = c(21, 22.8), cyl = c(6, 4))
summarise(d, m = mean(mpg))                                # resolves
summarise(d, m = mean(mgp))                                # typo, caught
my_mean <- function(df, var) summarise(df, m = mean({{ var }}))  # silent
```

ry reports RY010 for `mgp`.

rlang's `{{ }}` embrace, the `.data` / `.env` pronouns, `!!` / `!!!`,
and functions that defuse their own arguments (a parameter whose first
use is `enquo()` / `substitute()` / ...) are recognized, so wrapper
functions do not produce false unbound-variable reports. When the
masked data's schema is unknown, ry stays silent rather than guessing
at column candidates.

## Dumping inferred types

Use `ry dump-types types.R` to inspect bindings and inferred types as JSON.
Add `--position LINE:COL` to query the scope at a specific position.
See the [inferred types reference](types.md) for an example, output fields,
and scope limits.

## Exporting analysis facts

Use `ry dump-facts R/ --format json` to export versioned scope snapshots as
JSON. Add `--references` to include reference facts. See the
[structured facts reference](facts.md) for schemas, output fields, and limits.

## Editors

`ry server` speaks the Language Server Protocol over stdio: diagnostics
as you type (debounced, cached parses), inlay hints, and quick-fix
actions that insert suppression comments.

Diagnostics cover the whole project, using the same analysis as `ry check`. The
inlay hints and quick-fix actions apply to the open document only.

The [editor playground](../editors/example-project/README.md) has a small
package with expected diagnostics and steps for checking edits, hints,
and suppression actions in VS Code, Positron, or Zed.

### VS Code / Positron

Install the **ry** extension from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=sims1253.ry)
or Open VSX (for Positron). The extension bundles the `ry` binary.

See the [extension guide](../editors/code/README.md) for settings, commands,
and binary selection.

### Zed

Install the **R** extension for R language support, then install **ry**.
The ry extension uses a local `ry` executable or downloads one from GitHub
releases. To use ry as the R language server, add this to Zed's settings:

```json
{
  "languages": {
    "R": { "language_servers": ["ry"] }
  }
}
```

If you already use other R language servers, add `"ry"` to that list.

### Other editors (Neovim, Helix, Emacs)

Connect manually by pointing your LSP client at `ry server`. For
example, with Neovim's built-in LSP:

```lua
local root_marker =
  vim.fs.find({'ry.toml', 'DESCRIPTION', '.git'}, { upward = true })[1]

vim.lsp.start({
  name = 'ry',
  cmd = {'ry', 'server'},
  root_dir = root_marker and vim.fs.dirname(root_marker) or vim.fn.getcwd(),
})
```

## Known limits

ry rejects source whose tree-sitter syntax tree exceeds 128 nested levels,
with a parser error that identifies ry’s nesting limit and the source location.
This protects recursive analysis on ordinary worker-thread stacks; it is not
an R language limit. The bound measures syntax-tree depth, not line count or
file size; wide argument lists remain allowed.

When both operator operands have different S3 methods, ry can follow a
`chooseOpsMethod` returning literal `TRUE` or `FALSE` when the current scope
proves both methods and the chooser values in top-level code using ordinary
`<-` or `=` assignments. This includes aliases and methods with `...`; the
selected operator method must also return a literal. Other calls, uncertain rebinding, function bodies checked before
execution, attached packages, and more complex methods keep the result unknown.

For scalar primitive operands, ry reports RY051 when both choosers return
`FALSE` and different literal method result modes prove that the methods differ.
This also covers plain vectors built with proven base `structure()` calls whose
only attribute is a literal class and whose payload is a scalar literal or a
flat, unnamed `base::c()` call of atomic literals. Ordinary copies retain this
proof; writes and control-flow merges discard it. Arithmetic keeps the longer
operand's class (left on ties); comparison and logical results drop class.
Dimensions, names, other attributes, atomic empty vectors, and unknown lengths
remain outside the vector proof. Other fallback cases stay unknown.
See [#193](https://github.com/sims1253/ry/issues/193).

For example, this top-level sequence selects a character result:

```r
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
chooseOpsMethod.left <- function(...) TRUE
selected <- x + y
```

This proof assumes ordinary initial bindings, as reference facts do; it does
not establish the state of a pre-populated R session. An intervening unknown
call, such as `change_environment()`, makes subsequent selection unknown even
if the methods are assigned again: it could install a delayed binding. Putting
this sequence inside `function() { ... }` also leaves selection unknown because
the function can run after its environment changes.

S4 modeling covers in-package `setClass` / `setGeneric` /
`setMethod` and `@` slot access but not full method resolution order;
R6 modeling covers `self` / `private` / `super` in method bodies, not
field types. No expansion of dynamic `exportPattern()` directives and no
NA tracking yet. Cross-package names without stubs resolve to opaque
values when static package metadata proves that they exist.

## Building from source

Install Rust 1.88 or newer, then run:

```sh
git clone https://github.com/sims1253/ry
cd ry
cargo build --release
# binary at target/release/ry
```

See [Contributing](../CONTRIBUTING.md) for tests and development instructions.
For prebuilt binaries, see the [installation instructions](../README.md#install).
