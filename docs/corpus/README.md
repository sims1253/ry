# Ecosystem diagnostic ledgers

Each ledger pins the *identities* (package, rule code, path, line, column) of
every diagnostic `ry` emitted on an audited package corpus, together with the
independent classification (`true_positive` / `false_positive` / `uncertain`)
and an `audit_group` label. Pinning identities — not aggregate counts — means
removing one finding can never be silently mistaken for removing another.

The `audit_group` field records why the audit gave each label. Most groups
name a cause, such as `type-narrowing` or `test-fixture`. Others name an
upstream owner, such as `upstream-package` or `upstream-glue`, or a review
batch, such as `manual-audit` or `P2`.

| Ledger | `ry` | Packages | Diagnostics | TP / FP / Unc | Reconciliation |
| :-- | :-- | :-- | ---: | :-- | :-- |
| [`tidyverse-0.7.1.json`](tidyverse-0.7.1.json) | 0.9 dev | 24 | 82 | 37 / 29 / 0 (+16 unowned) | hermetic (strict CI gate) |
| [`posit-0.9.0.json`](posit-0.9.0.json) | 0.9 dev | 62 | 310 | 87 / 223 / 0 | hermetic (strict CI gate) |

The `0.7.1` in the tidyverse ledger's filename is historical. The `ry` column
gives the version that produced its diagnostics. The
[pre-governance measurement](pre-governance-measurement.md) records the 0.9
audit's starting point.

## Instruction counts

The [instruction baseline](instructions-baseline.json) records the fixed performance
sample. See the [measurement guide](instructions.md) for local runs and CI deltas.
The [statement-branch journal](scope-journal.md) records the production design
and its allocation measurements.
The [scope storage experiments](scope-journal-experiment.md) record journal
tradeoffs and persistent-map measurements for issue #130.

## Parser invariant evidence

[`parser-option-audit-0.9.md`](parser-option-audit-0.9.md) records the complete
parser-option audit of `?`, `.ok()?`, and `None` propagation. Its executable R1
and R6 gates live in `crates/ry-checker/tests/invariants.rs` and cover all
checker fixtures plus a deterministic sample of the vendored ecosystem sources.

## Readable message ledger

[`posit-messages-0.9.json`](posit-messages-0.9.json) records the message and
severity for every distinct identity in the reviewed Posit findings.
Each entry is keyed by the same stable `(package, code, path, line, column)`
identity as `posit-0.9.0.json`; it is intentionally readable JSON rather than a
digest or an ignored `.full.txt` report.

`ecosystem/run.sh` regenerates this companion from the production
`ry check --output-format json` results. Posit `--check` runs compare the
processed tier against the committed entries and print a unified diff for any
message or severity drift. A full non-check run updates all entries.

## Reconciliation

`ecosystem/run.sh` reconciles the hermetic root reports it generates
(`RY_NO_INSTALLED_LIBRARIES=1`) against a ledger. The ledger is the CI
baseline, and any missing or unowned identity fails the build. A disappeared `true_positive` gets its
own error line.

> Reproducing reports outside `run.sh` requires the same variable: without
> `RY_NO_INSTALLED_LIBRARIES=1` the checker consults the machine's installed R
> packages and produces different findings, so a hand-run diff against the
> committed reports will show phantom rows.

## Updating the ledgers

Checker and LSP changes may alter diagnostics intentionally. Any such change must
regenerate the reports, update every affected ledger (`posit-0.9.0.json`,
`tidyverse-0.7.1.json`) in the same change, preserve
or manually review every new identity's label, and explain all missing/unowned
identities. Never weaken `reconciliation: hermetic` to accept a delta.

The Posit ledger's `source_sha256` hashes the concatenated bytes of all
`ecosystem/reports/posit.*.root.txt` files, sorted by filename. Recompute it
after a full run. Package, classification, and audit-group counts must match
the reviewed identity list.

The tidyverse ledger's `source_sha256` uses the same recipe over every
non-`posit` `*.root.txt` report, one for each of the 32 entries in
`ecosystem/packages.txt`. The table's 24 packages count only the ledger's
`packages` entries. Eight manifest packages have no entry: the reports for
cli, rlang, and testthat hold 13 unowned findings, and those for curl, fs,
jsonlite, scales, and withr are empty.

## Running the corpora

```sh
# tidyverse (default; strict hermetic reconciliation)
ecosystem/run.sh --check

# posit corpus — strict 0.9 gate; fast tier (36 packages) or full (all 62)
ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --tier fast
ecosystem/run.sh --check --manifest ecosystem/posit-packages.txt --tier full
```

Manifests are collision-safe: each package is keyed by a unique slug and
`run.sh` aborts if a slug appears twice. A `# ledger:` directive in each
manifest selects its corpus, and a `# === full tier` marker separates the
fast-tier packages from the rest. Non-default manifests also namespace their
committed reports (for example `posit.glue.root.txt`) so packages pinned at
different commits never overwrite another corpus's baseline.

The package cache skips a fetch when the pinned commit is already present,
but still forces checkout of that pin and removes untracked files.
