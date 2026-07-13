# Plan 04: Data-driven package semantics (kill hardcoded checker knowledge)

## Status: blocked on plans 01 (module split) and 03 (runtime stubs).
## Repos: ry (this repo) + r-typeshed (schema + data changes, local checkout ../r-typeshed).

## Context

The checker hardcodes package-specific knowledge that should live in typeshed
data, so that package updates / new packages are data changes, not engine
changes. After plan 01 the relevant code lives in `crates/ry-checker/src/`
modules (s3.rs, nse.rs, higher_order.rs — names may differ slightly; locate
by symbol).

Hardcoded today (pre-split lib.rs line refs):

1. `S3_GENERICS` (~line 63), `S3_DENYLIST` (~line 91), `AMBIENT_GLOBALS`
   (~line 109) — pure data tables.
2. `NseVerb::from_name` (~line 279) — hardcoded base-R + dplyr NSE verbs
   (subset/with/within/transform/filter/mutate/summarise/select/arrange/
   group_by), each with a bespoke `infer_nse_*` method. Meanwhile the
   typeshed JSON already carries per-parameter `eval` modes
   (`data_mask`, `tidy_select`) that overlap with this.
3. `HigherOrderFunc` (~line 306) — hardcoded base-R apply family + purrr
   family with per-function `ho_*` handlers.
4. `infer_dplyr_join`, `infer_tidyr_pivot_call` — dplyr/tidyr specifics.

## Design

### Stage A: move the plain tables into base stub data

Extend the stub schema (schema_version stays "1"; all additions are optional
keys) with a top-level `globals` section in `base.json`:

```json
"globals": {
  "ambient": [".data", ".env", ".GlobalEnv", ...],
  "s3_generics": ["Ops", "Math", "print", "summary", ...],
  "s3_split_denylist": ["t.test", "all.equal", "file.path", ...]
}
```

ry-typeshed deserializes these into `Typeshed` (new optional fields,
defaulting to empty). The checker reads them from the loaded base typeshed
instead of the const arrays; delete the const arrays. Keep behavior
identical: same contents, same lookup semantics. A user-supplied base stub
override (plan 03) thereby also controls these tables — document that in
SCHEMA.md.

### Stage B: declarative schema effects for NSE verbs

Add an optional per-function `schema_effect` field to FunctionSig:

```json
"mutate": {
  "params": [".data", "..."],
  "return": "arg0",
  "eval": {"...": "data_mask"},
  "schema_effect": "add_named_args"
}
```

Enum values (cover exactly what `infer_nse_*` implements today — read each
method carefully before finalizing):

- `"preserve"` — result keeps arg0's schema/type (filter, arrange, group_by,
  subset without select).
- `"add_named_args"` — arg0 schema plus one column per named `...` argument,
  typed by inferring that argument in the data-mask scope (mutate,
  transform, within).
- `"select"` — schema reduced to the tidy-selected columns (select; subset's
  select argument).
- `"aggregate"` — fresh data.frame whose columns are the named `...`
  arguments (summarise).
- `"expression_value"` — result is the inferred type of the second argument
  evaluated in the data-mask scope (with).

The checker's NSE path becomes: (a) function's typeshed entry has any
`eval` mode of `data_mask`/`tidy_select` AND the package is loaded (existing
gating via `loaded` set, `crates/ry-checker` — keep the current gating
semantics exactly, including `tidyverse` implying dplyr) -> build the
augmented scope from arg0's column schema (existing `dplyr_data_mask_scope`)
-> apply `schema_effect` to compute the result type. The `NseVerb` enum and
`from_name` are deleted; base-R verbs (subset/with/within/transform) get
typeshed entries in base.json with the appropriate `eval` + `schema_effect`
fields (add them in the r-typeshed checkout AND re-vendor via
`scripts/sync_typeshed.sh`).

Keep genuinely bespoke logic in code where a declarative spec cannot express
it (e.g. `infer_dplyr_join`'s column union, tidyr pivot): those functions get
`schema_effect: "opaque_special"`? No — instead leave them on a small
hardcoded match as today, but move the DATA (which function names trigger
them) into the stub entries via `schema_effect: "join"` and
`schema_effect: "pivot"` values that dispatch to the existing Rust
implementations. The enum in Rust is the extension point; the JSON names it.

### Stage C: declarative higher-order specs

Add optional per-function `higher_order` object to FunctionSig:

```json
"map_dbl": {
  "params": [".x", ".f"],
  "return": {"mode": "double", "length": "unknown"},
  "higher_order": {
    "callback_param": ".f",
    "callback_position": 1,
    "callback_args": ["element_of_arg0"],
    "result": {"kind": "vector_of", "mode": "double"}
  }
}
```

`callback_args` values: `element_of_arg0`, `element_of_arg1`, `unknown`,
`accumulator_and_element` (Reduce). `result.kind` values:
`list_of_callback_return`, `vector_of` (with mode), `same_as_arg0` (keep/
discard/Filter), `callback_return` (Reduce/Find/do.call), `first_arg`
(walk), `simplify` (sapply-style: vector of callback return when scalar,
list otherwise), `fun_value_template` (vapply). Again: read every existing
`ho_*` handler and `HigherOrderFunc::from_call` case FIRST and make the
vocabulary cover them exactly; the purrr-gating (only when purrr loaded or
`purrr::`-qualified) must be preserved — it falls out naturally because the
spec now lives on the purrr package stubs, which only apply when purrr is
loaded/qualified. Base-R entries (lapply/sapply/vapply/Map/mapply/rapply/
Reduce/Filter/Find/Position/do.call) move to base.json stubs. Delete
`HigherOrderFunc` and `from_call`; keep the result-computation code, now
driven by the spec.

`in_parallel` keeps its transparent-wrapper behavior via
`result.kind: "callback_identity"` or an equivalent spec value.

### Ordering within this plan

Do stages strictly in order A -> B -> C, keeping `cargo test --workspace`
green after each stage. Each stage changes BOTH repos: schema/SCHEMA.md/
validate.py + data in ../r-typeshed, then re-vendor into ry
(`scripts/sync_typeshed.sh ../r-typeshed`), then checker changes in ry.

## Rules

- Diagnostic output over the corpus must not change: `cargo test -p
  ry-checker --test corpus --test vendor_snapshot` green with NO snapshot
  updates. If a discrepancy is genuinely a pre-existing bug, stop and report
  rather than silently changing behavior.
- The existing unit tests for NSE/higher-order behavior must keep passing
  (they may need mechanical updates if they constructed `NseVerb` /
  `HigherOrderFunc` directly, but assertions on diagnostics must be
  untouched).
- No mutating git commands in either repo; leave working trees dirty.
- r-typeshed's `scripts/validate.py` must be extended to validate the new
  schema fields, and `schema/typeshed.schema.json` + SCHEMA.md updated.
- No emojis.

## Acceptance criteria

- `rg "S3_GENERICS|S3_DENYLIST|AMBIENT_GLOBALS|NseVerb|HigherOrderFunc"
  crates/ry-checker/src` returns nothing (or only comments referencing the
  history).
- corpus + vendor_snapshot tests pass unchanged.
- `python3 scripts/validate.py` passes in ../r-typeshed.
- A new checker test proves extensibility: a user stub dir (plan 03
  mechanism) defining a fake package with a `data_mask` verb +
  `schema_effect: add_named_args` gets full NSE column resolution without
  any Rust change.
- `cargo clippy --workspace --all-targets`: no new warnings; `cargo fmt --check` passes.
