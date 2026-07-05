# Architecture

One page on how ry is put together, for contributors and for anyone
citing the methodology. For build and contribution mechanics see
`CONTRIBUTING.md`.

## Crate map

```
crates/
  ry-core      parsing and the type lattice (no diagnostics)
  ry-typeshed  embedded per-package function stubs (JSON)
  ry-checker   inference, rules, project-level checking, output formats
  ry-cli       the `ry` binary: check/watch/rule/server subcommands
  ry-lsp       language server (tower-lsp) on top of ry-checker
```

Dependencies point strictly downward: `ry-cli` and `ry-lsp` depend on
`ry-checker`, which depends on `ry-core` and `ry-typeshed`. `ry-core`
depends on nothing but tree-sitter.

## Data flow

```
source text
  -> tree-sitter-r parse tree          (ry-core::parser)
  -> owned AST (Stmt / Expr / Span)    (ry-core::ast, lossy on purpose:
                                        comments are kept separately for
                                        suppression handling)
  -> walk + infer                      (ry-checker: walk_stmt / infer)
  -> diagnostics                       (ry-checker::diagnostics)
  -> suppression + severity filter     (# ry: ignore comments, ry.toml)
  -> renderer                          (full/concise/json/github/gitlab/junit)
```

## The type lattice (ry-core::types)

An `RType` is a mode (what `typeof()` would say), a length, an optional
S3 class vector (first four entries), an optional column schema (data
frames and shaped lists), an optional inferred function signature, and
optional union members. Design commitments:

- **Unions, not coercion, at control-flow merges.** `if (p) 1L else "a"`
  is `union[integer, character]`, never a silently promoted character.
  Unions are capped at 4 members and collapse to unknown beyond that.
  All union construction goes through the checked `RType::union`
  constructor; a member-less union cannot be built.
- **Operations distribute over unions.** A binary op on a union errors
  only if every member pair errors; one valid member means silence.
- **Opaque is absorbing and permissive.** Whenever inference gives up it
  says `unknown`, and unknown never produces a diagnostic. This is the
  no-false-positives stance in type form: ry prefers missing a bug to
  inventing one.

## The checker (ry-checker)

`Checker` handles a single file. `Project` shares state across files in
three passes:

1. **Collect.** Every file's top-level function definitions (and S3
   method registrations) go into one shared `FnTable`. Later files win
   name collisions, mirroring `source()` order.
2. **Refine.** A fixpoint loop re-infers every function's return type
   against the shared table until nothing changes (with a depth cap).
   This is what makes cross-file return-type inference converge.
3. **Emit.** Each file is walked once more for diagnostics against the
   refined tables. The tables are shared behind `Arc` and this pass is
   read-only, so it runs file-parallel under rayon.

There is no incremental engine (no salsa). The LSP approximates
incrementality by caching parses and scopes per document version and
debouncing full-project rechecks; the batch CLI just recomputes.

Package awareness: `library()` / `require()` calls (and a `packages`
key in `ry.toml`) populate a loaded-package set, unioned across the
project. NSE verbs like `filter` only get dplyr semantics when dplyr is
loaded or the call is `dplyr::`-qualified; otherwise the name resolves
against base R.

## The typeshed (ry-typeshed)

Per-package JSON stubs embedded at compile time: `base.json` (base,
stats, utils -- always loaded), `dplyr.json`, `purrr.json`,
`mirai.json`, and `bayes.json` (a multi-package file for brms,
posterior, loo, bayesplot, cmdstanr, keyed `pkg.function`). Signatures
are intentionally underspecified -- return types may be concrete, or
abstract slots like `arg0` that the checker resolves per call site.

Two rules keep the typeshed honest:

- `scripts/audit_typeshed.R` asserts every declared name actually
  exists in R (run in the oracle CI job). Entries that cannot be
  verified do not ship.
- `scripts/gen_typeshed.R` drafts stub files from a package's exports;
  a human refines the return types. The generator is a curation aid,
  not an oracle.

## How ry validates its own claims

Three nets, all in CI:

- **The R oracle** (`crates/ry-checker/tests/oracle.rs` +
  `scripts/oracle_driver.R`). Each fixture in `testdata/oracle/`
  declares `# oracle: must-pass`, `must-flag`, or `known-gap <reason>`.
  The fixtures are executed by R itself -- in parallel, via
  `purrr::map()` + `purrr::in_parallel()` on mirai daemons, the same
  idiom ry models -- and ry must agree with R's verdict. A known-gap
  that silently closes fails the suite (stale tags cannot ship).
- **Vendored CRAN code** (`tests/vendor_snapshot.rs`). ry runs over the
  vendored sources of real packages (currently glue and purrr) and
  insta-snapshots every diagnostic. Each snapshot line is triaged in a
  comment block: true positive, or known limitation with the planned
  fix. A rule that dominates a snapshot is treated as broken. The glue
  snapshot is empty; purrr's remaining lines are all cross-package
  names (rlang/vctrs) that the typeshed does not cover yet.
- **The corpus** (`tests/corpus.rs` + `testdata/*.R`). Small fixtures,
  one behavior each: `ok_*` files must produce no diagnostics, others
  declare exactly which rule fires where.

## Adding a rule

1. Register it in `crates/ry-checker/src/rules.rs` (stable code, name,
   default severity, summary). Codes are lexicographic.
2. Emit it from the checker via `self.emit(...)`.
3. Add corpus fixtures for both directions: the bug it catches and the
   nearby idiom it must NOT flag.
4. If R can arbitrate the behavior, add an oracle fixture.
5. Re-run the vendor snapshots. If your rule fires on vendored CRAN
   code, every firing must be a defensible true positive -- otherwise
   the rule does not merge. This is the bar.
