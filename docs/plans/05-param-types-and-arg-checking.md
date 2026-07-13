# Plan 05: Parameter types in the stub schema + argument-checking diagnostics

## Status: blocked on plans 03, 04, and 08.

## NOTE (supersedes references below): r-typeshed no longer uses Python.
## Wherever this plan says to update `validate.py` or
## `schema/typeshed.schema.json`, instead: (a) the strict serde loader in
## ry-typeshed validates the new `params` object form by construction;
## (b) extend `ry typeshed validate` (added in plan 08) if convention-level
## checks are needed; (c) document the new fields in schema/SCHEMA.md only.
## Repos: ry (this repo) + r-typeshed (../r-typeshed).

## Context

`FunctionSig.params` is `Vec<String>` — parameter NAMES only. ry therefore
cannot check argument types or names against known signatures. This plan
extends the schema with optional parameter metadata and adds three new
diagnostics. This is the single largest new-diagnostics unlock and the main
step toward ty-class analysis.

## Schema extension (r-typeshed side)

Keep `params: [string]` valid forever (bare-name form). Additionally allow
each element to be an object:

```json
"params": [
  {"name": "x", "type": {"mode": "double", "length": "unknown"}, "required": true},
  {"name": "trim", "type": {"mode": "double", "length": "one"}, "default": true},
  "..."
]
```

- `name` (required). `"..."` stays a plain string or `{"name": "..."}`.
- `type` (optional RType, same shape as return types). Absent = unchecked.
- `required` (optional bool, default false): call must bind this parameter.
- `default` (optional bool): has a default value (informational; `required`
  false already implies callable without it).

Update `schema/typeshed.schema.json`, SCHEMA.md, `validate.py`, and
`gen_typeshed.R` (the generator can emit `required` from `formals()` —
a formal with no default and not `...` is required; it cannot infer types,
leave them absent).

Populate types for a HIGH-VALUE, LOW-RISK subset of base.json only (do not
attempt full coverage): `mean`, `sum`, `length`, `paste`/`paste0`, `nchar`,
`toupper`/`tolower`, `substr`, `grepl`/`sub`/`gsub` (pattern/x character),
`seq_len`, `rep`, `sqrt`/`log`/`exp`, `round`, `ifelse`, `stopifnot`,
`is.na`, `as.integer`/`as.numeric`/`as.character` — approximately 30
functions. For each, be deliberately permissive: only declare a param type
when passing another mode is ALWAYS a bug at runtime or statistically wrong
(e.g. `mean(x)` with character input is an error in R -> declare
`{"mode": "double"}`? NO — mean accepts logical/integer/double/complex and
date types. Use mode unions where needed, see below, or leave untyped).
When in doubt, leave the type off. False positives are worse than missed
checks.

Mode unions: if the RType `mode` field cannot express "numeric-like",
check how `Mode::Union`/`members` works in `crates/ry-core/src/types.rs`
and how JSON stubs would express it. If the JSON side has no union support,
add `"mode": "union"` + `"members": ["logical","integer","double"]` to
JsonRType handling in ry-typeshed and the conversion code (find where
JsonRType -> RType conversion happens; likely ry-checker). Alternatively, if
this is too invasive, support a shorthand `"mode": "numeric"` meaning
logical|integer|double at conversion time — pick whichever fits the existing
type lattice more cleanly and document the choice.

## ry side: serde + conversion

- `ry-typeshed`: `params` becomes `Vec<ParamSpec>` with a custom
  deserializer accepting string or object (untagged enum). Add
  `ParamSpec { name, type_?, required, default }`. Existing callers using
  `params()` as names get a `param_names()` helper to stay compiling.
- Conversion of param `type` JSON to `RType` reuses the exact code path used
  for return types.

## New diagnostics (ry-checker)

Register in `crates/ry-checker/src/rules.rs` (keep codes lexicographic; RY09x
block appears unused — verify with `rg "RY09" crates/` first, else pick the
next free block):

1. **RY090 unknown-argument** (warning): a named argument in a call does not
   match any parameter name of the resolved signature (after R's partial
   matching rules: a named arg matches a param if it is an exact match or an
   unambiguous prefix of exactly one param; `...` in the signature swallows
   any non-matching named args — in that case NEVER fire). Fires only when
   the signature has no `...`.
   Include a did-you-mean hint when a parameter name is within Levenshtein
   distance 2 (implement a small edit-distance fn or use an existing dep —
   check Cargo.toml/Cargo.lock for something already in-tree before adding
   any dependency; if adding one is needed, prefer implementing ~20 lines of
   Levenshtein inline instead).
2. **RY091 missing-required-argument** (warning): a param with
   `required: true` is bound by neither position nor name. Respect R
   argument matching: exact names first, then partial names, then positional
   fill-in. Implement matching as a reusable function with thorough unit
   tests — this is subtle; follow R semantics precisely (named args are
   matched and REMOVED from the positional queue, then positionals fill
   remaining params in order, `...` absorbs the rest).
3. **RY092 argument-type-mismatch** (error): an argument bound to a param
   with a declared `type` has an inferred RType whose mode is known
   (not Unknown/Opaque) and is incompatible. Reuse the compatibility ladder
   the checker already uses for RY040 arithmetic (find `coerce_rank` /
   compatibility logic in ry-core types.rs) — logical/integer/double are
   mutually compatible (R coerces); character vs numeric is incompatible;
   list/function vs atomic is incompatible. Only fire on KNOWN
   incompatibility; any Unknown/Opaque/union-overlap = silence.

All three fire only when the signature resolves from typeshed (embedded or
user stubs) or from a user-defined function in the FnTable (for user
functions: params are known from the definition; all params without defaults
are required; no types). Gate carefully on `...` handling as above.

Wire severity filtering/suppression automatically (existing rule registry
mechanics should cover it — verify `ry explain rule RY090` works and
`# ry: ignore[RY090]` suppresses).

## Tests

- Unit tests for the argument-matching function (exact, partial, ambiguous
  partial, positional after named, `...` absorption).
- Corpus fixtures in `crates/ry-checker/testdata/`: at least 8 new fixtures
  covering each rule firing and each deliberate non-firing case (partial
  match ok, `...` swallows, unknown type silent, user fn required arg).
- vendor_snapshot: run it; NEW diagnostics on the vendored glue sources are
  expected. Triage each new diagnostic in the snapshot's comment block per
  the existing convention (true positive vs limitation). If any is a false
  positive, fix the rule before accepting the snapshot.
- e2e: one CLI test showing RY092 in `full` output format with the type in
  the message ("argument `x` to `mean` is `character`, expected numeric").

## Rules

- False-positive aversion is the prime directive. Every rule must have an
  explicit "when NOT to fire" list implemented and tested.
- No mutating git commands. No emojis. `uv`/`bun` conventions do not apply
  here (pure Rust + JSON).
- Do not add crate dependencies without strong need.
- Update README.md rule table (if present) and CHANGELOG.md.
- r-typeshed changes must pass its `validate.py`; re-vendor via
  `scripts/sync_typeshed.sh ../r-typeshed`.

## Acceptance criteria

- `cargo test --workspace` green; new fixtures pass; vendor_snapshot
  accepted with triage comments and zero untriaged false positives.
- `ry explain rule RY090/91/92` renders.
- Suppression comments work for the new codes (test).
- `cargo clippy` no new warnings; `cargo fmt --check` passes.
