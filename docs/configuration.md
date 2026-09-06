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

# Include R fixture data nested under package tests/ directories.
check-test-fixtures = false

# Bounded directory discovery. Both CLI (`ry check`) and LSP apply the
# same limits so the two modes discover exactly the same file set.
# Each key accepts a positive integer; zero is a configuration error.
[index]
max-files      = 20000   # files discovered per root (default: 20,000)
max-file-bytes = 2097152 # bytes per R file (default: 2 MiB)
max-depth      = 64      # directory depth (default: 64)
```

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
scope effects. See the [r-typeshed schema reference](https://github.com/sims1253/r-typeshed/blob/main/schema/SCHEMA.md)
when writing custom stubs.

## Inline suppression

``` r
x <- bad  # ry: ignore                 # suppress all rules on this line
x <- bad  # ry: ignore[RY010, RY040]   # suppress specific rules
x <- bad  # noqa: RY010                # flake8/ruff-compatible alias

# ry: ignore                           # standalone: suppresses the next line
# ry: ignore-file                      # file-level, anywhere in the file
```

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
removed by regenerating the baseline.
