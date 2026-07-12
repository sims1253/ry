# Plan 02: Bootstrap the r-typeshed repository

## Status: ready for implementation
## Repo: /home/m0hawk/Documents/r-typeshed (EMPTY git repo, branch `master`).
## Read-only reference: /home/m0hawk/Documents/ry (the ry checker workspace).

## Context

ry (an R static type checker in Rust, modeled on astral's ty) currently embeds
hand-curated JSON "typeshed" stubs at compile time from
`ry/crates/ry-typeshed/data/*.json`. We are extracting that data into this
standalone repo — analogous to `python/typeshed` — so stubs can be iterated,
reviewed, and released independently of the checker. ry will later vendor a
pinned snapshot of this repo (plan 03, separate task) and support loading
extra stub directories at runtime.

## Source material (copy from, do not modify, the ry repo)

- `ry/crates/ry-typeshed/data/base.json` (~64KB: base R functions, datasets,
  s3_methods)
- `ry/crates/ry-typeshed/data/{dplyr,purrr,mirai,survival,testthat}.json`
- `ry/crates/ry-typeshed/data/bayes.json` — multi-package file whose function
  keys are prefixed `brms.*`, `posterior.*`, `loo.*`, `bayesplot.*`,
  `cmdstanr.*`
- `ry/scripts/gen_typeshed.R` — draft-stub generator (runs against an
  installed package, emits skeleton JSON)
- `ry/scripts/audit_typeshed.R` — verifies stub entries against an installed
  package (no fabricated names)
- Schema shape: see `ry/crates/ry-typeshed/src/lib.rs` (structs `Typeshed`,
  `FunctionSig`, `JsonRType`, `ReturnSpec`, `ReturnSlot`, `EvalMode`,
  `RawS3Method`). The serde definitions there are the normative schema.

## Target repository layout

```
r-typeshed/
  README.md
  LICENSE                  (MIT, copy ry/LICENSE and keep the author line)
  CHANGELOG.md             (start with an "Unreleased: initial import" entry)
  schema/
    typeshed.schema.json   (JSON Schema draft 2020-12 for one stub file)
    SCHEMA.md              (human-readable schema documentation)
  stubs/
    base/base.json
    dplyr/dplyr.json
    purrr/purrr.json
    mirai/mirai.json
    survival/survival.json
    testthat/testthat.json
    brms/brms.json         (split out of bayes.json, see below)
    posterior/posterior.json
    loo/loo.json
    bayesplot/bayesplot.json
    cmdstanr/cmdstanr.json
  scripts/
    gen_typeshed.R
    audit_typeshed.R
    validate.py            (new: validates every stubs/**/*.json against the
                            JSON Schema; stdlib-only Python or use `check-jsonschema`)
  .github/workflows/ci.yml (validation on every PR)
```

## Tasks

1. **Copy the JSON data** into `stubs/<pkg>/<pkg>.json`.
2. **Split `bayes.json`**: it is one file with `pkg.function` prefixed keys
   for brms, posterior, loo, bayesplot, cmdstanr. Produce five per-package
   files with UNPREFIXED function keys (`brm`, not `brms.brm`). Write a
   throwaway script for the split (keep it in scripts/ as
   `split_multipackage.py` for provenance, or delete after use — your call).
   Preserve every field verbatim (`params`, `return`, `aliases`, `eval`,
   `source_relative_path_arg`), key order sorted lexicographically.
   IMPORTANT subtlety: entries in bayes.json under one prefix must only land
   in that package's file. `version` field: keep the source file's version
   string in each output.
3. **Add a `package` and `schema_version` header field** to every stub file:
   ```json
   {
     "schema_version": "1",
     "package": "dplyr",
     "version": "<keep existing version value>",
     "functions": { ... },
     "datasets": { ... },
     "s3_methods": [ ... ]
   }
   ```
   Keep all existing keys unchanged otherwise. (`base.json` gets
   `"package": "base"`.) ry's loader will be taught about the two new keys in
   plan 03; unknown keys are already ignored by serde there, so this is
   forward-compatible.
4. **Write `schema/typeshed.schema.json`** describing the full file shape:
   - `schema_version` (string, required), `package` (string, required),
     `version` (string, required)
   - `functions`: map name -> { `params`: [string], `return`: ReturnSpec,
     `aliases`?: [string], `eval`?: map param -> one of `normal`,
     `quoted_symbol`, `quoted_expression`, `data_mask`, `tidy_select`,
     `source_relative_path_arg`?: integer }
   - ReturnSpec: either the string-slot object form `"arg0"` /
     `"concat_of_args"` (exact serde representation: check how `ReturnSlot`
     serializes — it is `#[serde(rename = "arg0")]` on unit variants inside an
     untagged enum, so the JSON is the bare string `"arg0"`) or an RType
     object: { `mode`: string, `length`: string, `na`?: bool,
     `class`?: [string], `columns`?: map name -> RType }.
   - `datasets`: map name -> RType.
   - `s3_methods`: array of { `generic`: string, `class`: string, plus the
     FunctionSig fields flattened }.
   Derive the exact permitted values for `mode`/`length` from real usage:
   `python3 -c` over all JSON files to enumerate the distinct values used,
   and cross-check with `ry/crates/ry-core/src/types.rs` (`Mode`, `Length`
   parsing — find the code that converts `JsonRType.mode`/`length` strings
   into `Mode`/`Length`, likely a `from_str`/match in ry-core or ry-checker).
   Document each value in SCHEMA.md.
5. **Write `scripts/validate.py`**: validates all stub files against the
   schema AND enforces repo conventions: file path matches `package` field,
   function keys sorted (warn only), no duplicate alias collisions within a
   package. Exit non-zero on violation. Prefer stdlib-only (write a small
   validator for the subset of JSON Schema you need, or vendor
   `jsonschema` via `uv run --with jsonschema`).
6. **CI workflow** (`.github/workflows/ci.yml`): on push/PR, run
   `scripts/validate.py` (use `uv` if you need third-party packages:
   `uvx --from jsonschema ...` or `uv run --with jsonschema
   scripts/validate.py`). Optionally a second job that runs
   `audit_typeshed.R` for packages available in a rocker container — mark it
   `continue-on-error: true` since CRAN installs are slow; keep it minimal
   (base only) or skip if too heavy.
7. **README.md**: what this repo is (typeshed for the R ecosystem, consumed
   by ry), the file format (link SCHEMA.md), how to add a package (workflow:
   `gen_typeshed.R` draft -> hand-curate -> `audit_typeshed.R` ->
   `validate.py` -> PR), versioning policy (schema_version bumps only on
   breaking schema changes; repo tagged releases are what ry vendors),
   relationship to ry/air/jarl.
8. **Update scripts' header comments** (`gen_typeshed.R`) so the emitted
   draft includes the new header fields (`schema_version`, `package`).

## Rules

- Do NOT modify anything in /home/m0hawk/Documents/ry — it is reference only.
- Do NOT run any git commands (no commits). Leave everything as untracked
  working-tree files; the user commits.
- Data fidelity is paramount: after the split/headers, a round-trip check
  must prove no function entry was lost or altered. Write a small comparison
  script that, for each source ry JSON, checks every function key exists in
  exactly one output file with an identical body (modulo the new header
  keys). Run it and include it in scripts/ as `verify_import.py`.
- No emojis anywhere.

## Acceptance criteria

- `python3 scripts/validate.py` exits 0.
- `python3 scripts/verify_import.py /home/m0hawk/Documents/ry/crates/ry-typeshed/data` exits 0,
  proving lossless import.
- All 11 stub files exist with correct `package` fields.
- Schema + SCHEMA.md cover every construct actually used in the data (no
  "additionalProperties: true" escape hatches on the function entry level).
- CI workflow YAML is syntactically valid (`uvx --from yamllint yamllint` or
  careful review).
