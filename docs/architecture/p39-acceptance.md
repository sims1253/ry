# P39: External Semantic Catalog — Acceptance Record

## Workstream status

| WS | Status | Description |
|---|---|---|
| W1 | ✅ | Design: structured effect enum (Design A) |
| W2 | ✅ | r-typeshed-schema crate v0.1.0, tagged schema-v0.1.0 |
| W3 | ✅ | Validator + compiled pack format (CatalogPack) |
| W4 | ✅ | Catalog adapter: typeshed→FunctionSemantics IR conversion |
| W5 | ✅ | Effect interpreter: lookup_call(), is_defusing(), is_predicate() |
| W6 | ✅ | Custom catalog layering: layer_catalogs() |
| W7 | ✅ | Rule registry + Markdown table generator |
| W8 | ✅ | r-typeshed schema-crate committed with .gitignore, CI-ready |
| W9 | ⬜ | Precision program (future: measure editor profiles) |
| W10 | ✅ | This acceptance record |

## Deliverables

### In r-typeshed repo
- `schema-crate/` — independently versioned authoring contract
  - `TypeshedDocument`, `FunctionSignature`, `ParameterDef`, `RTypeSpec`
  - `EvalMode`, `ReturnSpec`, `PredicateDef`, `AssertionDef`
  - `validate()` function
  - `CatalogPack`, `compile_document()`, `build_pack()` — deterministic pack format
  - 5 tests, tagged schema-v0.1.0

### In ry repo
- `crates/ry-analysis/src/catalog.rs` — neutral FunctionSemantics IR
  - Evaluation, FlowEffect, ReturnRule, BindingEffect, Dispatch
  - SemanticCatalog trait, InMemoryCatalog
- `crates/ry-analysis/src/catalog_adapter.rs` — typeshed→IR conversion
  - `convert_function_sig()`, `catalog_from_typeshed()`
- `crates/ry-analysis/src/effect.rs` — one effect interpreter
  - `lookup_call()` (issue #49: catalog query seam)
  - `is_defusing()`, `is_data_mask()` (issue #41: one NSE encoding)
  - `is_predicate()`, `is_assertion()`, `is_no_return()` (issue #40: semantic flags)
- `crates/ry-analysis/src/layering.rs` — custom catalog support
  - `layer_catalogs()` — official + user catalogs with override semantics
- `crates/ry-analysis/src/rules.rs` — generated rule documentation
  - `all_rules()`, `rules_markdown_table()` — single source of truth

## Issues closed
- **#40**: Semantic flags accessible via catalog (is_predicate, is_assertion, etc.)
- **#41**: One NSE/defusing encoding via Evaluation enum
- **#49**: Catalog query seam via lookup_call()

## Remaining work
- W9 (precision program): Measure editor profiles with the catalog-driven system
- Full migration of checker internals to use the effect interpreter
- The catalog adapter and effect interpreter provide the seam; the checker's
  infer modules need to call through it instead of reading raw typeshed fields
