# Usage

[Getting started](../README.md) · [Configuration](configuration.md) · [Rules](rules.md)

- [Checking files and CI](#checking-files-and-ci)
- [R Markdown and Quarto inputs](#r-markdown-and-quarto-inputs)
- [Package awareness](#package-awareness)
- [Box imports](#box-imports)
- [uvr project libraries](#uvr-project-libraries)
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

To check an unsaved source buffer, pass `-` explicitly and give its logical
filename:

```sh
printf '%s\n' '"a" + 1L' | ry check - --stdin-filename R/example.R --output-format json
```

ry treats the buffer as the file at `R/example.R`. It uses that path for
config discovery, package context, baselines, and diagnostic paths, and checks
it even if `exclude` would skip it. The buffer replaces a disk file at that
path. The parent directory must exist, but the file need not. ry never writes
the buffer to disk. Other operands are checked as usual, and `--explain-files`
names the buffer on stderr.

The buffer decodes like a disk file. A failed read or a buffer over
`index.max-file-bytes` fails the check with an empty machine-readable report,
unless `--exit-zero` is set. `-` without `--stdin-filename`,
`--stdin-filename` without `-`, a repeated `-`, and `-` with `--watch` are
usage errors.

The exit status is nonzero when an error remains after filtering.
`--error-on-warning` also fails on warnings; `--exit-zero` lets the check
succeed despite findings. Usage failures return nonzero; check read and
discovery failures follow the `--exit-zero` policy.
Human-readable diagnostics use ANSI color on terminals; select the
color policy with `--color auto|always|never`. Automatic color respects
`NO_COLOR`, and machine-readable formats never contain ANSI escapes.

### JSON output and related locations

`--output-format json` prints an array of objects with `code`, `severity`,
`message`, `path`, `line`, `column`, `confidence`, and an optional `related`
array. Lines and columns are 1-based; columns count Unicode scalar values.
`related` is omitted when empty. Each item has a `message` and, when its span
fits the checked source, the `path`, `line`, and `column` of the location;
ry never guesses a location. A diagnostic carries at most three items.

RY090 and RY091 relate a call to a project function to the formal it names.
They do so only when the call provably reaches that definition: the exact
literal bound in the same file, with no unproven call, default argument, or
alias that could have rebound it. Cross-file calls carry no related locations.
`full` output prints each item as an indented `note:` line. `concise`,
`github`, `gitlab`, and `junit` show only the primary location. Related items
never change messages, baselines, suppressions, or the exit status. The
language server reports them as `relatedInformation` to clients that support
it.

### CI

`--output-format github` emits GitHub Actions annotations. Use `gitlab` for
GitLab Code Quality reports or `junit` for JUnit XML reports. `--statistics` prints
per-rule counts after a run. Once ry is installed, a GitHub Actions step
can run:

``` yaml
- run: ry check --output-format github .
```

To pin a release in CI, install it with that release's installer
(`https://github.com/sims1253/ry/releases/download/v<version>/ry-cli-installer.sh`).

For a pre-commit or prek hook, run ry once over the whole project rather than
per file, so a change to `ry.toml`, a baseline, or `NAMESPACE` is also checked:

``` yaml
repos:
  - repo: local
    hooks:
      - id: ry-project
        name: ry project check
        entry: ry check .
        language: system
        pass_filenames: false
        always_run: true
```

## R Markdown and Quarto inputs

ry can check the R chunks of `.Rmd` and `.qmd` reports in `ry check` and the
language server. This is opt-in:

```toml
[reports]
enabled = true
```

ry never renders or runs a report. It checks backtick-fenced R chunks
(`` ```{r} `` or `` ```{R} ``) in document order, so a chunk can use names
defined by earlier active chunks in the same report; each report has its own
environment. Findings use the report's original lines and columns. Prose,
YAML, non-R fences, tilde fences such as `~~~{r}` (knitr never runs them),
and `{{r}}` examples are ignored. Report extensions match in any case
(`.Rmd`, `.RMD`, `.qmd`, `.QMD`).

- **Execution options.** Literal `eval = FALSE`, `#| eval: false`, or
  `#| eval = FALSE` disables a chunk; `include` and `echo` do not. R-style
  options must use uppercase `TRUE`/`FALSE`; YAML `#|` options accept YAML
  booleans. Option names are case-sensitive.
- **RY120** marks input ry cannot extract: a malformed or unclosed R fence,
  unclosed YAML metadata block, or a report over the static limits (2 MiB,
  128 R chunks, or an R header over 16 KiB or 128 fields).
- **RY121** marks execution ry cannot determine statically: computed,
  conflicting, unsupported, or unreadable chunk options (such as `child`, or
  a `#|` line that is neither `key: value` nor `key = value`), real
  references to `knitr::opts_chunk`, and an R chunk header inside a fence
  knitr does not treat as a chunk (such as `~~~` or ```` ````markdown ````)
  stop analysis at that chunk. Engine
  selectors (`engine`, `jupyter`) and `execute`/`knitr` settings in any YAML
  metadata block, including under `format`, stop it for the whole report.
- A syntax error inside a chunk is reported as RY000. Each chunk parses on its
  own, so one chunk cannot complete another's expression.
- Inline R, child documents, notebooks, knitr hooks, and project-level
  renderer options are out of scope. Code actions that edit source are not
  offered for reports.

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

Vignette `.R` files are included unless a discovery rule excludes them.
`ry check . --explain-files` prints the included files and skipped paths on
stderr. A skipped directory stands for its whole subtree, and ry does not scan
it. Diagnostics, including JSON, stay on stdout. To check one ignored file,
pass its path directly. To always check a file that `.Rbuildignore` excludes,
list it in [`include-build-ignored`](configuration.md#project-configuration).

Typed purrr maps check the callback's return type. For example, save this as
`parallel.R` and run `ry check parallel.R`:

```r
library(purrr)
bad <- map_dbl(1:4, function(i) as.character(i))
```

ry reports RY080 because the callback returns character values where `map_dbl`
requires doubles. `in_parallel()` preserves the callback's inferred type.

Calls to a project function whose whole body is one formal, or one base
operation on one formal, are checked against that body:

```r
f <- function(x) x + 1L
f("oops")      # RY092: `x` is an operand of `+`
nm <- function(df) df$value * 2
nm(1:3)        # RY092: `$` is invalid for atomic vectors
id <- function(x) x
id("a") + 1    # RY040: `id` returns its argument
```

Only unbraced, unparenthesized bodies qualify, because `{`, `(` and `return`
are ordinary bindings in R. The function must be the only literal bound to its
name, and no project binding or import may replace the operator. Classed and
`NULL` actuals stay quiet. Any call anywhere in the project that can rebind
names in an existing frame (`source()`, `load()`, `eval()`, `list2env()`,
`.GlobalEnv`, and the other entries of `BINDING_INSTALLERS`) switches these
checks off for the whole project.

## Box imports

ry reads static `box::use()` imports without loading a module or running R.
`box::use(./mod/hello)` binds `hello`; `box::use(./mod/hello[say_hello])`
binds only `say_hello`, plus the module object when the import has an
explicit alias. `[...]` attaches the known exports except originals renamed
in the same selection. Package imports such as `dplyr[filter]` use the
installed `NAMESPACE` exports and ry's bundled stubs without attaching the
package to the search path. Search-path modules such as `mod/hello` bind
opaque names; ry does not resolve `box.path`, so their wildcards leave
unenumerated names unknown.

Relative modules resolve from the importing file, preferring open editor
buffers, and try `.r`, `.R`, `__init__.r`, then `__init__.R` appended to the
full module name (`foo.bar` becomes `foo.bar.r`). An existing preferred file
that cannot be analyzed stays opaque rather than falling back to a later
candidate.

Exports follow box 1.2.3. Literal `box::export()` declarations override
roxygen `@export` tags. A tag applies when it ends its line in the comment
region before a declaration; blank lines, ordinary comments, and multiple
`#` prefixes may intervene. A tagged `box::use()` exports its module alias
and selected names, while a tagged wildcard leaves the inventory
incomplete. A module with neither exports its own non-dot top-level
bindings, including module objects but not selectively attached names.

RY118 reports a selected name or `$` member that a complete local inventory
proves missing. Computed imports, dynamic exports, module-load calls that may
write bindings, unreadable or deeply nested modules, and package metadata
stay silent. Exported functions keep their formals; when a module imports
names at top level, their return types stay unknown.

## uvr project libraries

[uvr](https://github.com/nbafrank/uvr/blob/main/README.md) installs a project's
packages in `.uvr/library/`. Provision that library with uvr first. Then pass
its path to ry through the existing `R_LIBS` environment variable. From the
project directory, use this POSIX shell recipe:

```sh
uvr sync
R_LIBS="$PWD/.uvr/library" ry check .
```

In PowerShell on Windows, use:

```powershell
uvr sync
$env:R_LIBS = Join-Path (Get-Location) '.uvr\library'
ry check .
```

ry only reads static package metadata from that library; it does not read uvr
lockfiles, run `.Rprofile`, or execute package code. For the editor, set the
same `R_LIBS` in the language server's environment (for example by launching
the editor from that shell) and restart the server after changing it.

An installed package is not automatically attached. ry uses its `NAMESPACE`
exports when checked source calls `library()` or `require()`, or when a source
package imports it. A `uvr.toml` or `uvr.lock` dependency alone adds no
unqualified names; declare a package attached by the host with
`packages = ["name"]` in `ry.toml`.

ry searches a nearby `renv/library` first, then `R_LIBS`, `R_LIBS_USER`,
`R_LIBS_SITE`, and the standard R library locations; the first `NAMESPACE`
found for a package wins. `ry dump-facts --format json .` records a context
hash for the library inventory, which helps when CLI and editor results
differ.

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

See the [extension guide](../editors/code/README.md) for installation,
settings, commands, and binary selection.

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

ry uses the published tree-sitter-r grammar, which requires adjacent closing
brackets in double subscripts: write `x[[i]]`, not `x[[i] ]`. R also accepts
newlines and comments between those closing brackets, but ry's parser does not.
These spellings can cause false diagnostics or prevent useful diagnostics when
parser recovery loses surrounding code. We accept this upstream limitation
instead of maintaining a separate grammar patch.

ry rejects source whose tree-sitter syntax tree exceeds 128 nested levels,
with a parser error that identifies ry’s nesting limit and the source location.
This protects recursive analysis on ordinary worker-thread stacks; it is not
an R language limit. The bound measures syntax-tree depth, not line count or
file size; wide argument lists remain allowed.

When two operands have different S3 methods, ry follows `chooseOpsMethod()`
only when both methods and the choosers it consults are top-level `<-` or `=`
assignments that return literals. Aliases and methods with `...` count. Any other call,
uncertain rebinding, attached package, or more complex method leaves the result
unknown ([#193](https://github.com/sims1253/ry/issues/193)). For example, this
top-level sequence selects a character result:

```r
x <- structure(1L, class = "left")
y <- structure(2L, class = "right")
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) 1L
chooseOpsMethod.left <- function(...) TRUE
selected <- x + y
```

An unknown call such as `change_environment()` between these lines leaves the
result unknown, even if the methods are assigned again, because the call could
install a delayed binding. The same code inside `function() { ... }` also
leaves it unknown. ry assumes ordinary initial bindings, not a pre-populated R
session.

RY051 reports two operands whose choosers both return `FALSE` and whose methods
return different literals, because R then warns and uses the primitive
operator. Both operands must be length-one logical, integer, double, or
character values. Longer operands count only when a base `structure()` call
whose only attribute is a literal class builds them from a flat, unnamed
`base::c()` of atomic literals. Copies keep this proof. Writes and control-flow
merges drop it.

S4 modeling covers in-package `setClass` / `setGeneric` /
`setMethod` and `@` slot access. S4 dispatch selects a project method only
when the call provably reaches it: an exact full-signature match, or a
single-dispatch generic with an exact receiver class; otherwise the result
is unknown. Class inheritance is not modeled. R6 modeling covers `self` / `private` / `super` in method bodies, not
field types. No expansion of dynamic `exportPattern()` directives and no
NA tracking yet. Cross-package names without stubs resolve to opaque
values when static package metadata proves that they exist.

## Building from source

Install Rust 1.90 or newer, then run:

```sh
git clone https://github.com/sims1253/ry
cd ry
cargo build --release
# binary at target/release/ry
```

See [Contributing](../CONTRIBUTING.md) for tests and development instructions.
For prebuilt binaries, see the [installation instructions](../README.md#install).
