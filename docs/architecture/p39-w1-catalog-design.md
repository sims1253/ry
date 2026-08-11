# P39-W1: Semantic Catalog Design

## Two design alternatives

### Design A: Structured effect enum (chosen)

The neutral IR uses a structured enum for `SemanticEffect`, where each variant
represents a well-defined runtime behavior. This is the "make impossible states
impossible" approach.

```rust
pub enum Evaluation {
    Eager,
    PromiseCapture { defusing: DefusingKind },
    Quoted,
    DataMask { select: Option<SelectKind> },
}

pub enum FlowEffect {
    None,
    Predicate { targets: PredicateTarget },
    Assertion { stop_on_fail: bool },
    NoReturn,
}

pub enum ReturnRule {
    Type(RTypeExpr),
    Conditional { branches: Vec<(Condition, RTypeExpr)> },
    HigherOrder { map_over: usize },
    FirstArg,
}

pub struct FunctionSemantics {
    pub parameters: Vec<ParameterSpec>,
    pub evaluation: Evaluation,
    pub return_rule: ReturnRule,
    pub flow_effect: FlowEffect,
    pub binding_effect: Option<BindingEffect>,
    pub dispatch: Option<Dispatch>,
}
```

**Strengths:** exhaustiveness checking, no flag bags, compiler can validate
consistency.

**Weaknesses:** more types to define, less flexible for ad-hoc additions.

### Design B: Flat field record with validation

Each function is a flat record with optional fields, validated by a rules
engine.

```rust
pub struct FunctionSemantics {
    pub eval_mode: Option<EvalMode>,
    pub predicate: Option<bool>,
    pub assertion: Option<bool>,
    pub return_type: Option<String>,
    pub higher_order: Option<usize>,
    // ... 15+ optional fields
}
```

**Strengths:** simple structure, easy to author.

**Weaknesses:** flag bag, no exhaustiveness, easy to create inconsistent
combinations (predicate=true AND assertion=true makes no sense).

## Decision: Design A

Design A's structured approach prevents impossible states and leverages
Rust's type system for validation. The compiler in r-typeshed validates
authoring input and emits the structured IR. This aligns with Plan 38's
"deep module" principle and the semantic-list coherence discipline (P35-W7).

## Seam: ry catalog adapter

The adapter lives in `crates/ry-analysis/src/catalog.rs` and implements:

```rust
pub trait SemanticCatalog {
    fn lookup(&self, function: &str) -> Option<FunctionSemantics>;
    fn package_functions(&self, package: &str) -> &[FunctionEntry];
}
```

The interpreter converts `FunctionSemantics` into the checker's runtime
effects (type narrowing, promise capture, etc.) through one entry point,
replacing the scattered field reads across infer modules.
