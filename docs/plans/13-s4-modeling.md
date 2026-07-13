# Plan 13: Minimum-viable S4 modeling + named-vector plumbing

## Status: implemented (2026-07-12).

Derived from ROADMAP item 11 (Partial: group-generic `.Generic` binding is
done) and amendment B — S4 gaps caused ry's worst wrong-schema FP cluster
(terra `divide.R`, 16 sites: schema inference did not know the in-package
`setMethod("as.vector", "SpatExtent")` returns a NAMED vector) plus
Matrix's 157 diags (`@` slot access unmodeled) and DBI's 28 (cross-file
`setGeneric`/`setMethod`).

## Scope — files this plan may touch

- `crates/ry-checker/src/collect.rs`
- `crates/ry-checker/src/infer/index.rs`
- `crates/ry-checker/src/infer/call.rs`
- `crates/ry-checker/src/infer/misc.rs`
- `crates/ry-checker/src/lib.rs` (checker state for the S4 tables)
- `crates/ry-checker/src/tests.rs`
- `crates/ry-checker/testdata/` (new fixtures)

## Part A — collect in-package S4 definitions

Mirror the existing S3 collection (`collect.rs` builds `s3_methods`; follow
that pattern):

- `setClass("C", representation(a = "numeric", ...))` and the
  `slots = c(a = "numeric")` form: record class name -> slot names (+ the
  declared slot class strings, kept as opaque class tags).
- `setGeneric("gen", ...)`: record `gen` as a defined function-name (kills
  "unbound" on cross-file uses).
- `setMethod("gen", "Class", function(...) ...)` and
  `setMethod("gen", signature("Class", ...), ...)`: record
  (generic, first-signature-class) -> the method function, in a new
  `s4_methods` table shaped like `s3_methods`.
- All of this must work cross-file in project mode — check how `s3_methods`
  flows through `project.rs`/`lib.rs` today and ride the same path. Do not
  edit project.rs unless the existing flow genuinely does not carry the new
  tables; if you must, keep the edit minimal.

## Part B — dispatch through `s4_methods`

Where the checker resolves a call to a generic whose receiver's class is
known (see `try_s3_dispatch` in `infer/index.rs` or wherever it lives),
also consult `s4_methods`: if the receiver carries class "C" and
(gen, "C") is recorded, use that method's inferred return type.

Acceptance shape (the terra regression, distilled):

```r
setClass("SpatExtent", representation(ptr = "numeric"))
setMethod("as.vector", "SpatExtent", function(x, mode = "any") {
  c(xmin = 1, xmax = 2, ymin = 3, ymax = 4)
})
f <- function(e) {
  v <- as.vector(e)   # e : SpatExtent
  v[["xmin"]]          # must NOT be flagged: the vector is named
}
```

A call `as.vector(e)` on a known-`SpatExtent` value must pick up the named
return schema so `v[["xmin"]]` / `v["xmin"]` produce no diagnostic.

## Part C — `@` slot access

- Handle the `@` operator in `infer/index.rs` (check how the parser
  represents it — likely an `IndexKind` or a distinct expr; if the parser
  does not produce it at all, check ry-core and report back rather than
  patching the parser in this plan).
- `obj@slot` where `obj`'s class has recorded slots: known slot -> opaque
  typed with the slot's class tag; unknown slot on a KNOWN class -> keep
  silent for now (no new rule in this plan). Unknown class -> opaque,
  silent. `@` must never produce RY010/unknown-field noise.
- `obj@slot <- value` assignment form must also be accepted.

## Part D — named-vector plumbing through `t()` and `data.frame()`

Amendment B's related fix: propagate vector names through `t()` and
`data.frame(matrix)` so named-vector -> data.frame schemas are right in
both directions. Concretely: `data.frame(t(v))` and `as.data.frame(t(v))`
where `v` has known names should produce a schema with those column names
(and `$name` access on the result stays clean). Check `infer/construct.rs`
/ `call.rs` for where `data.frame` schemas are built today.

## Tests / acceptance

- Unit tests for Parts A–D including the distilled terra shape above as a
  named fixture (this is the negative-canary from the audit: 16 FPs today
  must go to 0), cross-file setGeneric/setMethod in a two-file project
  test (DBI shape), `@` access on declared and undeclared slots, and the
  `data.frame(t(named))` schema.
- `cargo fmt`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace` all green.
- No git state-changing commands; preserve pre-existing uncommitted
  changes. Only touch Scope files; report anything else instead of editing.
