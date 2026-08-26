# Ecosystem diagnostic ledgers

Each ledger pins the *identities* (package, rule code, path, line, column) of
every diagnostic `ry` emitted on an audited package corpus, together with the
independent classification (`true_positive` / `false_positive` / `uncertain`)
and a `workstream` label. Pinning identities — not aggregate counts — means
removing one finding can never be silently mistaken for removing another.

| Ledger | `ry` | Packages | Diagnostics | TP / FP / Unc | Reconciliation |
| :-- | :-- | :-- | ---: | :-- | :-- |
| [`tidyverse-0.7.1.json`](tidyverse-0.7.1.json) | 0.7.1 | 24 | 100 | 4 / 96 / 0 | hermetic (strict CI gate) |
| [`posit-0.9.0.json`](posit-0.9.0.json) | 0.9 dev | 62 | 728 | 37 / 691 / 0 | hermetic (strict CI gate) |

Two historical ledgers were removed as generated artifacts: the 0.8.0
audit transcript (1,142 identities, reconciliation `audit-transcript`) and
the pre-change baseline (729 identities, hermetic, measured on the
audited starting tree). Neither gated CI; both are re-derivable from the
audit records summarized in [`plan34-measurement.md`](plan34-measurement.md).

## Parser invariant evidence

[`parser-option-audit-0.9.md`](parser-option-audit-0.9.md) records the complete
parser-option audit of `?`, `.ok()?`, and `None` propagation. Its executable R1
and R6 gates live in `crates/ry-checker/tests/invariants.rs` and cover all
checker fixtures plus a deterministic sample of the vendored ecosystem sources.

## Readable message ledger

[`posit-messages-0.9.json`](posit-messages-0.9.json) records the message and
severity for all 728 reviewed Posit diagnostics. It previously also carried an
optional structured fix; the autofix machinery was removed before 0.9.0 (see
issue #89), so those payloads are gone.
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

- **`hermetic`** (default when absent; every committed ledger). The ledger *is*
  the hermetic CI baseline: any missing or unowned identity fails the build.
- **`audit-transcript`** (no committed ledger uses it today; previously the
  removed 0.8.0 posit ledger). A ledger transcribed from an *installed-library*
  audit run legitimately differs under a hermetic rerun, so the missing/unowned
  delta is printed for visibility but does not gate the build. The mode is kept
  and exercised by `ecosystem/test-reconciliation.R` in case a future corpus is
  transcribed from an audit rather than measured hermetically.

In both modes, findings labeled `true_positive` are checked explicitly so a
real bug disappearing is always surfaced.

## The moving 0.9 ledger

The audit response rebuilt `ry` from the audited starting commit, generated hermetic
message-free root reports for all 62 pinned packages, and measured the
pre-change baseline (see [`plan34-measurement.md`](plan34-measurement.md);
the intermediate snapshot itself was removed as a generated artifact). After
the RY032 cleanup produced a zero identity delta, that exact full run seeded
`posit-0.9.0.json`, which — unlike those removed historical snapshots — is a
strict gate.

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
