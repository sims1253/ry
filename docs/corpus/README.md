# Ecosystem diagnostic ledgers

Each ledger pins the *identities* (package, rule code, path, line, column) of
every diagnostic `ry` emitted on an audited package corpus, together with the
independent classification (`true_positive` / `false_positive` / `uncertain`)
and a `workstream` label. Pinning identities — not aggregate counts — means
removing one finding can never be silently mistaken for removing another.

| Ledger | `ry` | Packages | Diagnostics | TP / FP / Unc | Reconciliation |
| :-- | :-- | :-- | ---: | :-- | :-- |
| [`tidyverse-0.7.1.json`](tidyverse-0.7.1.json) | 0.7.1 | 24 | 100 | 4 / 96 / 0 | hermetic (strict CI gate) |
| [`posit-0.8.0.json`](posit-0.8.0.json) | 0.8.0 | 62 | 1142 | 34 / 1108 / 0 | audit-transcript (informational) |

## Reconciliation modes

`ecosystem/run.sh` reconciles the hermetic root reports it generates
(`RY_NO_INSTALLED_LIBRARIES=1`) against a ledger. The ledger's `reconciliation`
field selects how a delta is treated:

- **`hermetic`** (default when absent; the tidyverse ledger). The ledger *is*
  the hermetic CI baseline: any missing or unowned identity fails the build.
- **`audit-transcript`** (the posit ledger). The ledger was transcribed from an
  *installed-library* audit run, so a hermetic rerun legitimately differs. The
  missing/unowned delta is printed for visibility but does **not** gate the
  build; re-audit and regenerate the ledger to update it.

In both modes, findings labelled `true_positive` are checked explicitly so a
real bug disappearing is always surfaced.

## Regenerating the posit ledger

The posit ledger is generated — never hand-written — from the audit working
directory, so the 1,142 identities and the 34/1108 classification are re-derived
from the audited artefacts every time. The generator lives with the audit data
(`ry-audits/posit-corpus/transcribe-corpus.R`) and is deliberately not vendored
here: it reads a private audit checkout, and this repo carries only the
resulting ledger.

```
# from the audit checkout
Rscript transcribe-corpus.R --ry-repo /path/to/ry           # write docs/corpus/posit-0.8.0.json
Rscript transcribe-corpus.R --ry-repo /path/to/ry --check   # assert the committed file matches
```

`--ry-repo` defaults to `$RY_REPO`, else the `ry` directory next to the audits
root, so a sibling checkout needs no arguments.

The script reads `packages.json`, `aggregate.json` and, per package,
`audit-results/<slug>/{ry-stdout.json,summary.json,git_commit}`. It joins every
`ry` diagnostic to its classification on (code, path, line, message) and
**asserts the invariants** before writing: the 62 pinned package slugs, 1,142
diagnostics, and exactly 34 true positives / 1,108 false positives / 0
uncertain, with no orphan diagnostics and no conflicting labels (so no package
is ever silently skipped). `--check` compares the complete regenerated corpus
against the committed file byte for byte.

The tidyverse ledger has no generator in this repo either.

## Running the corpora

```
# tidyverse (default; strict hermetic reconciliation)
ecosystem/run.sh --check

# posit corpus — fast tier (35 signal-dense packages) or full (all 62)
ecosystem/run.sh --manifest ecosystem/posit-packages.txt --tier fast
ecosystem/run.sh --manifest ecosystem/posit-packages.txt --tier full
```

Manifests are collision-safe: each package is keyed by a unique slug and
`run.sh` aborts if a slug appears twice. A `# ledger:` directive in each
manifest selects its corpus, and a `# === full tier` marker separates the
fast-tier packages from the rest.
