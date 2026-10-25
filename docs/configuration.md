# Configuration

[Getting started](../README.md) · [Usage](usage.md) · [Rules](rules.md)

- [Project configuration](#project-configuration)
- [Custom typesheds](#custom-typesheds)
- [Inline suppression](#inline-suppression)
- [Confidence tiers and baselines](#confidence-tiers-and-baselines)

## Project configuration

`ry check` finds `ry.toml` by walking up from the checked path. All keys are optional:

``` toml
# Promote / demote / disable rules by code (RY040), name
# (invalid-arithmetic), or "all".
error  = ["RY040"]
warn   = ["RY070"]
ignore = ["RY033"]

# Add rules to the default rule set, or replace that set with `select`.
extend-select = ["RY003"]
# select      = ["RY010", "RY040"]

# Packages attached outside the checked sources.
packages = ["dplyr"]

# Names created dynamically by the host application or an unresolvable
# load(). Only these names are treated as opaque globals.
globals = ["runtime_data", "generated_lookup"]

# Additional package stubs. Paths are relative to this ry.toml.
typeshed = ["stubs", "../shared-r-stubs"]

# Accepted findings from `ry check --write-baseline`; new findings still fail.
baseline = "ry-baseline.json"

error-on-warning = false
exit-zero        = false
output-format    = "full"     # full | concise | json | github | gitlab | junit

# gitignore-style patterns, relative to this ry.toml's directory.
exclude = ["renv", "tests/snaps/**"]

# Check these source files even when .Rbuildignore excludes them.
include-build-ignored = ["vignettes/benchmark.R"]

# Include R fixture data nested under package tests/ directories.
check-test-fixtures = false

# Decoded-byte cap per serialized R data file (R/sysdata.rda, data/*.rda,
# load() targets). Files above the cap fall back to a file-stem binding and
# are reported as degraded scopes.
max-serialized-bytes = 16777216 # 16 MiB (default)

# Bounded directory discovery. Both CLI (`ry check`) and LSP apply the
# same limits so the two modes discover exactly the same file set.
# Each key accepts a positive integer; zero is a configuration error.
[index]
max-files      = 20000   # files discovered per root (default: 20,000)
max-file-bytes = 2097152 # bytes per R file (default: 2 MiB)
max-depth      = 64      # directory depth (default: 64)

# Static report input is opt-in. It covers ordinary fenced R chunks only.
[reports]
enabled = false
```

`include-build-ignored` holds globs relative to `ry.toml`. CLI discovery and
editor indexing check matching files even when `.Rbuildignore` excludes them.
The setting does not override `exclude`, fixture settings, symlink rules,
hidden or generated directories, or resource limits. Use `vignettes/**` to
include every file in that directory. `vignettes` alone matches only the
directory. To see which files a check includes and skips, run
`ry check . --explain-files` ([usage](usage.md#package-awareness)).

Serialized R data files are inventoried by decoding at most
`max-serialized-bytes` bytes; one further byte is read to detect overflow. The
16 MiB default covers real package sysdata such as gt's ~8 MB table bundle. A
file above the cap falls back to a file-stem binding. The CLI and LSP report
these files as degraded scopes. Set the cap from 1 byte through 268435456 bytes
(256 MiB); zero does not mean unlimited.

The parser also limits nested parsing calls to 64 and element storage to
128 MiB per collection. These limits are separate from `max-serialized-bytes`
and are not a total memory budget. A file that hits one falls back to a
file-stem binding, as with the byte cap. Malformed, unsupported, unreadable, or
undecodable input binds no enumerated names, but a `data/` file still binds its
file stem. All of these cases report a degraded scope and leave diagnostics in
other files unchanged. The CLI prints the notices on stderr, and `dump-facts`
lists their paths and reasons in context inputs.

Use an environment profile for bindings supplied only to selected files:

```toml
[[environments]]
name = "shiny"
bindings = ["input", "output", "session"]
paths = ["inst/shiny/**"]
```

Profile paths are glob patterns relative to the configuration directory, even
when checking a nested package. `*` stays within one path component; `**`
includes descendants. Use forward slashes on every platform. Existing paths
are resolved through symlinks before matching.

To adopt the audited `typehint` 0.1.0 comments only in selected source files:

```toml
[annotations.typehint]
adopt = true
version = "0.1.0"
paths = ["R/**"]
```

All three settings are required when adoption is enabled. `paths` matches like
environment profile paths. No comment is adopted by default. The
[declaration guide](declarations.md) explains the
supported class subset, partial records, and diagnostics. Quarto cell options (`#| key:`),
ordinary comments, and strings do not create typehint contracts.

Explicit CLI values override scalar settings such as `output-format`.
The `--error`, `--warn`, `--ignore`, and `--typeshed` lists append to the
configuration lists. Rule filters apply errors first, then warnings, then
ignores. For example, `ignore = ["RY010"]` still suppresses RY010 when you
pass `--error RY010`; remove the ignore entry to enable that rule.
When multiple paths are checked, the first path anchors config discovery;
that one configuration applies to the complete invocation.

To change only the severity reported for selected source files, add ordered
`[[rule-overrides]]` tables:

```toml
warn = ["RY040"]

[[rule-overrides]]
paths = ["R/**"]
error = ["RY040"]

[[rule-overrides]]
paths = ["R/scratch/**"]
warn = ["RY040"]
```

Here RY040 is an error in `R/`, a warning in `R/scratch/`, and a warning
elsewhere. Each table requires `paths` and accepts `error`, `warn`, and
`ignore` lists by code, name, or `"all"`. `paths` matches like environment
profile paths.

The last matching table to mention a rule wins; within one table, `ignore`
wins over `error`, which wins over `warn`. Unmentioned rules keep the global
policy, and a globally ignored rule stays disabled. Explicit CLI
`--error`/`--warn`/`--ignore` and editor `lint.*` lists take priority over
path tables for the rules they name. Overrides change reported findings only;
every file still contributes bindings and types to project analysis.

In the editor, open documents that are ineligible for analysis (excluded by
`exclude` patterns, over `max-file-bytes`, or below a pruned `max-depth`)
may still receive syntax highlighting and editor features, but they do not
enter project-wide binding or diagnostic state.

## Custom typesheds

Custom stub directories let a project add package signatures or replace ry's
vendored signatures without recompiling. Both `stubs/foo.json` and
`stubs/foo/foo.json` layouts are accepted. The optional `package` header names
the package; legacy files fall back to the JSON file stem.

Directories are layered in declaration order and `--typeshed <DIR>` may be
repeated to append CLI directories. Later directories win, so CLI stubs replace
same-named config stubs. A custom package replaces the embedded package as a
whole; function-by-function merging is intentionally not performed. A
`base.json` stub likewise replaces the embedded base typeshed for that run.
Malformed files produce a warning naming the file while valid siblings remain
active.

Run `ry explain typeshed` to see the vendored snapshot, embedded packages, and
the custom directories active from the current workspace's `ry.toml`.

ry accepts schema 1 and schema 2 stubs. Schema 2 can describe predicates,
assertions tied to specific package functions, return lengths, and conditional
scope effects. See the [r-typeshed schema reference](https://github.com/sims1253/r-typeshed/blob/master/schema/SCHEMA.md)
when writing custom stubs.

## Inline suppression

``` r
x <- bad  # ry: ignore                 # suppress all rules on this line
x <- bad  # ry: ignore[RY010, RY040]   # suppress specific rules
x <- bad  # ry: ignore[]               # legacy alias for all rules
x <- bad  # noqa: RY010                # flake8/ruff-compatible alias

# ry: ignore                           # standalone: suppresses the next line
# ry: ignore-file                      # file-level, anywhere in the file
```

Selective `ry: ignore[...]` lists must contain registered `RY` codes; an
unknown code or malformed bracket produces RY112 at the comment and
suppresses nothing. Text after the codes is treated as explanation. A `noqa`
list may name other tools' codes; ry uses only its registered `RY` entries,
so a list without any suppresses nothing in ry. Bare `ry: ignore` and bare
`noqa` suppress all rules on their target line.

Enable the unused-ignore audit with `--warn RY113` or `warn = ["RY113"]`. It
reports a code in a valid `ry: ignore[...]` comment when the checker found no
such finding on the target line, before any filtering. It currently audits
RY034 and RY102, whose premises are local syntax; files with parse errors,
bare ignores, `noqa`, and file ignores are not audited. An ignore comment
cannot hide its own RY112 or RY113 finding; disable those rules by
configuration instead.

For dynamic workspaces, use a rule-specific inline suppression or a `globals`
entry. ry still reports findings inside `expect_error()`, because the code
there can hold a real defect before the expected error. Findings inside an
error probe -- `expect_error()`, `expect_warning()`,
`expect_snapshot(error = TRUE, ...)`, `try()`, or `tryCatch(..., error = ...)`
-- are demoted one confidence tier instead (see below), so
`--min-confidence medium` hides them without a suppression comment. Only the
probe argument is demoted; the wrapper's other arguments and code around it
keep full confidence, and a locally defined `expect_error()` is never treated
as testthat's.

## Confidence tiers and baselines

Every diagnostic carries a confidence tier. Structurally exact rules
(such as RY093, RY094, RY096, RY030, and RY033) are `high`; RY010 is `medium`;
diagnostics from `tests/`, `data-raw/`, `demo/`, `vignettes/`, and
`inst/` are demoted one tier, as are diagnostics inside an error-assertion
probe (`expect_error()`, `expect_snapshot(error = TRUE, ...)`, `try()`,
`tryCatch(..., error = ...)`). Output is sorted by tier, non-medium
tiers are tagged in the message, and `--min-confidence high|medium|low`
filters both output and exit code.

To adopt ry on an existing codebase, snapshot the current findings and
fail only on new ones:

``` bash
ry check --write-baseline ry-baseline.json .
ry check --baseline ry-baseline.json .    # or `baseline` in ry.toml
```

Baseline entries match on path, rule, and message, not line. A finding that
moves stays baselined. To drop fixed findings, rerun `--write-baseline`. It
writes the current findings without subtracting the configured `baseline`.
