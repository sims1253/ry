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
```

`include-build-ignored` contains glob patterns relative to `ry.toml`. It
lets CLI discovery and editor indexing check selected files excluded by
`.Rbuildignore`. It does not override `exclude`, fixture settings, symlink
rules, hidden or generated directories, or resource limits.

Use `vignettes/**` to include source files throughout that directory;
`vignettes` alone matches only the directory, not its files.

Run `ry check . --explain-files` to see included files and skipped paths on
stderr. A skipped directory represents its whole subtree; ry does not scan it
to count the files inside. Diagnostic output, including JSON, stays on stdout.

Serialized R data files are inventoried by decoding at most
`max-serialized-bytes` bytes; one further byte is read to detect overflow. The
16 MiB default covers real package sysdata such as gt's ~8 MB table bundle. A
file above the cap falls back to a file-stem binding. The CLI and LSP report
these files as degraded scopes. Set the cap from 1 byte through 268435456 bytes
(256 MiB); zero does not mean unlimited.

The parser also limits nested parsing calls to 64 and checks up to 128 MiB of
materialized element storage per collection, including metadata read in lazy
mode. These are separate from the decoded-file cap and are not a total memory
budget. A parser resource-limit failure reports a degraded scope and uses the
same file-stem fallback as the decoded-byte cap. Malformed, unsupported,
unreadable, or undecodable serialized input also reports a degraded scope and contributes no
enumerated bindings. The `data/` convention still contributes the file stem
when enumeration yields no names. A valid empty workspace reports no degradation. These
outcomes do not disable diagnostics in other files; the CLI keeps notices on
stderr, and `dump-facts` includes their paths and reasons in context inputs.

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
are resolved through symlinks before matching. A programmatically constructed
configuration without a file location uses the analysis root.

To adopt the audited `typehint` 0.1.0 comments only in selected source files:

```toml
[annotations.typehint]
adopt = true
version = "0.1.0"
paths = ["R/**"]
```

All three settings are required when adoption is enabled. The path globs use
the same configuration-root-relative physical path matching described above.
The matcher refuses native filenames it cannot represent as UTF-8; a lossy
display name cannot make an excluded file eligible. No comment is adopted by
default. The [declaration guide](declarations.md) explains the supported class
subset, partial records, and diagnostics. Quarto cell options (`#| key:`),
ordinary comments, and strings do not create typehint contracts.

Explicit CLI values override scalar settings such as `output-format`.
The `--error`, `--warn`, `--ignore`, and `--typeshed` lists append to the
configuration lists. Rule filters apply errors first, then warnings, then
ignores. For example, `ignore = ["RY010"]` still suppresses RY010 when you
pass `--error RY010`; remove the ignore entry to enable that rule.
When multiple paths are checked, the first path anchors config discovery;
that one configuration applies to the complete invocation.

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

Selective `ry: ignore[...]` lists must contain registered `RY` codes.
The older unbracketed code-list form (`ry: ignore RY040 RY010`) is also
selective when its first word resembles a rule code. Commas may have spaces
on either side. Code-like words before trailing prose must name registered
rules; after the first ordinary word, the rest is explanation and may itself
mention rule codes. The colon form
requires only code tokens. Brackets make the boundary between codes and an
explanation explicit.
Unknown codes and malformed brackets produce RY112 at the comment and do
not suppress findings. A `noqa` list can also name another tool's codes;
ry uses only its registered `RY` entries. A foreign-only list suppresses
nothing in ry. Bare `ry: ignore` (including explanatory prose) and bare
`noqa` suppress all rules on their target line. Any nonempty text after
`noqa` is interpreted as a code list; if it contains no registered `RY`
codes, it suppresses nothing in ry. Put explanatory prose after a native
`ry: ignore` instead. `ignore[ ]` is an alias for
`ignore[]`. Use `--ignore RY112` or the corresponding severity
configuration to disable directive validation; a bare ignore cannot hide
its own RY112 finding.

Enable the unused-ignore audit with `--warn RY113` or `warn = ["RY113"]` in
`ry.toml`. It checks valid `ry: ignore[...]` comments, including standalone
ones, one code at a time. The initial audit covers RY034 and RY102, whose
premises are local syntax; ignores for inference-dependent rules remain
unaudited until the checker can prove their analysis was complete. A code is
considered used if the checker found it on the target line before inline
suppression, severity filtering, baseline subtraction, or confidence
thresholds. Disabled rules, files with parse errors, excluded files, bare
ignores, `noqa`, and file ignores receive no unused finding. Anonymous
function bodies and unmodeled expression regions are unaudited; direct named
function bodies remain eligible. RY113 points to the comment. Use
`--ignore RY113` or `ignore = ["RY113"]` to disable it;
an inline ignore cannot hide the audit itself.

Prefer a rule-specific inline suppression or `globals` entry for dynamic
workspaces. ry intentionally does not suppress diagnostics merely because an
expression appears inside `expect_error()`: the setup expression is ordinary R
code and can contain a real defect before the expected error is reached.

## Confidence tiers and baselines

Every diagnostic carries a confidence tier. Structurally exact rules
(such as RY093, RY094, RY096, RY030, and RY033) are `high`; RY010 is `medium`;
diagnostics from `tests/`, `data-raw/`, `demo/`, `vignettes/`, and
`inst/` are demoted one tier. Output is sorted by tier, non-medium
tiers are tagged in the message, and `--min-confidence high|medium|low`
filters both output and exit code.

To adopt ry on an existing codebase, snapshot the current findings and
fail only on new ones:

``` bash
ry check --write-baseline ry-baseline.json .
ry check --baseline ry-baseline.json .    # or `baseline` in ry.toml
```

Baseline entries match on path, rule, and message. They ignore line numbers,
so moving a finding to another line does not invalidate it. Fixed findings can be
removed by regenerating the baseline. Regeneration snapshots what the check
currently reports — the configured `baseline` is not loaded or subtracted
first — so regenerating on an unchanged project leaves the file as it is
while findings that no longer occur drop out.
