# Ecosystem diagnostic ledgers

Each ledger pins the *identities* (package, rule code, path, line, column) of
every diagnostic `ry` emitted on an audited package corpus, together with the
independent classification (`true_positive` / `false_positive` / `uncertain`)
and a `workstream` label. Pinning identities — not aggregate counts — means
removing one finding can never be silently mistaken for removing another.

| Ledger | `ry` | Packages | Diagnostics | TP / FP / Unc | Reconciliation |
| :-- | :-- | :-- | ---: | :-- | :-- |
| [`tidyverse-0.7.1.json`](tidyverse-0.7.1.json) | 0.7.1 | 24 | 100 | 4 / 96 / 0 | hermetic (strict CI gate) |
| [`posit-0.8.0.json`](posit-0.8.0.json) | 0.8.0 | 62 | 1142 | 34 / 1108 / 0 | audit-transcript (historical) |
| [`posit-plan34-baseline.json`](posit-plan34-baseline.json) | pre-0.9 | 62 | 729 | 37 / 692 / 0 | hermetic (measured starting tree) |
| [`posit-0.9.0.json`](posit-0.9.0.json) | 0.9 dev | 62 | 729 | 37 / 692 / 0 | hermetic (strict CI gate) |


## Parser invariant evidence

[`parser-option-audit-0.9.md`](parser-option-audit-0.9.md) records the complete
P35-W4 audit of parser `?`, `.ok()?`, and `None` propagation. Its executable R1
and R6 gates live in `crates/ry-checker/tests/invariants.rs` and cover all
checker fixtures plus a deterministic sample of the vendored ecosystem sources.

## Readable message and fix ledger

[`posit-messages-0.9.json`](posit-messages-0.9.json) records the message,
severity, and optional structured fix for all 729 reviewed Posit diagnostics.
Each entry is keyed by the same stable `(package, code, path, line, column)`
identity as `posit-0.9.0.json`; it is intentionally readable JSON rather than a
digest or an ignored `.full.txt` report.

`ecosystem/run.sh` regenerates this companion from the production
`ry check --output-format json` results. Posit `--check` runs compare the
processed tier against the committed entries and print a unified diff for any
message or replacement drift. A full non-check run updates all entries.

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

```sh
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

## The moving 0.9 ledger

Plan 34 rebuilt `ry` from the audited starting commit and generated hermetic
message-free root reports for all 62 pinned packages. The complete pre-change
measurement is retained in `posit-plan34-baseline.json`; after the RY032 cleanup
produced a zero identity delta, that exact full run seeded `posit-0.9.0.json`.
Unlike the historical 0.8 audit transcript, the 0.9 ledger is a strict gate.

Plans 35 and 36 may change diagnostics intentionally. Any such change must
regenerate the reports, update `posit-0.9.0.json` in the same change, preserve
or manually review every new identity's label, and explain all missing/unowned
identities. Never weaken `reconciliation: hermetic` to accept a delta.

## Running the corpora

```sh
# tidyverse (default; strict hermetic reconciliation)
ecosystem/run.sh --check

# posit corpus — strict 0.9 gate; fast tier (35 packages) or full (all 62)
ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --tier fast
ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --tier full
```

Manifests are collision-safe: each package is keyed by a unique slug and
`run.sh` aborts if a slug appears twice. A `# ledger:` directive in each
manifest selects its corpus, and a `# === full tier` marker separates the
fast-tier packages from the rest. Non-default manifests also namespace their
committed reports (for example `posit.glue.root.txt`) so packages pinned at
different commits never overwrite another corpus's baseline.
