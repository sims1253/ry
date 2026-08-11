//! Semantic catalog adapter and effect interpreter.
//!
//! P39-W1/W4: This module defines the neutral semantic IR that bridges
//! the external r-typeshed catalog and ry's internal type system.
//!
//! Design: Structured effect enum (Design A from P39-W1).
//! Each variant represents a well-defined runtime behavior, making
//! impossible states unrepresentable.

use std::collections::HashMap;

// == Evaluation mode ==

/// How function arguments are evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Evaluation {
    /// Arguments are evaluated eagerly (default R behavior).
    Eager,
    /// Arguments are captured as unevaluated promises.
    PromiseCapture {
        /// What kind of defusing/capture this performs.
        defusing: DefusingKind,
    },
    /// Arguments are quoted (substitute/bquote).
    Quoted,
    /// Arguments are evaluated in a data mask.
    DataMask {
        /// Whether tidy-select semantics apply.
        select: Option<SelectKind>,
    },
}

/// Kind of promise defusing/capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefusingKind {
    /// Captures the expression without the environment.
    Enquo,
    /// Captures the expression with the environment.
    Enquos,
    /// Captures as a symbol.
    Ensym,
}

/// Kind of tidy-select semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    /// Selects columns from a data frame.
    ColumnSelect,
    /// Selects and renames.
    ColumnRename,
}

// == Flow effect ==

/// Control-flow effect of a function call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowEffect {
    /// No special flow effect (default).
    None,
    /// Narrows types based on the predicate (e.g., is.numeric).
    Predicate {
        /// What the predicate targets for narrowing.
        target: PredicateTarget,
    },
    /// Halts execution on failure (e.g., stopifnot).
    Assertion {
        /// Whether to stop on the first failure.
        stop_on_fail: bool,
    },
    /// Never returns (e.g., stop, quit).
    NoReturn,
}

/// What a predicate narrows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateTarget {
    /// Narrows the first argument.
    FirstArg,
    /// Narrows all arguments.
    AllArgs,
}

// == Return rule ==

/// How a function's return type is determined.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnRule {
    /// Always returns the given type.
    Fixed(String),
    /// Returns the type of the Nth argument (1-indexed).
    NthArg(usize),
    /// Returns the first non-null argument.
    FirstNonNull,
    /// Higher-order: maps over the Nth argument.
    HigherOrder { map_over: usize },
    /// Conditional return based on arguments.
    Conditional,
    /// Cannot determine statically.
    Unknown,
}

// == Binding effect ==

/// Side-effect of binding/assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingEffect {
    /// Injects bindings into the calling environment.
    Inject(Vec<String>),
    /// Performs assignment (assign, <-).
    Assign,
    /// Loads a package.
    Load,
    /// Sources a file.
    Source,
}

// == Dispatch ==

/// Method dispatch semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch {
    /// S3 generic, optionally in a group.
    S3 { group: Option<String> },
    /// S4 generic.
    S4,
    /// Replacement function (xxx<-).
    Replacement,
}

// == Parameter spec ==

/// Specification of a function parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterSpec {
    /// Parameter name.
    pub name: String,
    /// Whether this parameter has a default value.
    pub has_default: bool,
    /// Whether this is a variadic (...) parameter.
    pub variadic: bool,
}

// == Function semantics ==

/// Complete semantic description of a function.
///
/// This is the neutral IR that the catalog compiler emits and the
/// ry catalog adapter interprets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionSemantics {
    /// Parameter specifications.
    pub parameters: Vec<ParameterSpec>,
    /// How arguments are evaluated.
    pub evaluation: Evaluation,
    /// Return type rule.
    pub return_rule: ReturnRule,
    /// Control-flow effect.
    pub flow_effect: FlowEffect,
    /// Optional binding side-effect.
    pub binding_effect: Option<BindingEffect>,
    /// Optional method dispatch.
    pub dispatch: Option<Dispatch>,
}

impl Default for FunctionSemantics {
    fn default() -> Self {
        Self {
            parameters: Vec::new(),
            evaluation: Evaluation::Eager,
            return_rule: ReturnRule::Unknown,
            flow_effect: FlowEffect::None,
            binding_effect: None,
            dispatch: None,
        }
    }
}

// == Catalog trait ==

/// A semantic catalog that provides function semantics.
///
/// The catalog adapter in ry implements this trait, backed by either
/// the compiled r-typeshed pack or a custom user catalog.
pub trait SemanticCatalog {
    /// Look up semantics for a function by fully-qualified name.
    fn lookup(&self, function: &str) -> Option<&FunctionSemantics>;

    /// List all functions in a package.
    fn package_functions(&self, package: &str) -> Vec<&str>;

    /// Number of known functions.
    fn function_count(&self) -> usize;
}

// == In-memory catalog ==

/// A simple in-memory catalog for testing.
#[derive(Debug, Default)]
pub struct InMemoryCatalog {
    functions: HashMap<String, FunctionSemantics>,
}

impl InMemoryCatalog {
    /// Create a new empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a function's semantics.
    pub fn register(&mut self, name: &str, semantics: FunctionSemantics) {
        self.functions.insert(name.to_string(), semantics);
    }
}

impl SemanticCatalog for InMemoryCatalog {
    fn lookup(&self, function: &str) -> Option<&FunctionSemantics> {
        self.functions.get(function)
    }

    fn package_functions(&self, package: &str) -> Vec<&str> {
        self.functions
            .keys()
            .filter(|name| name.starts_with(&format!("{}::", package)))
            .map(|s| s.as_str())
            .collect()
    }

    fn function_count(&self) -> usize {
        self.functions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_register_and_lookup() {
        let mut cat = InMemoryCatalog::new();
        cat.register(
            "base::length",
            FunctionSemantics {
                parameters: vec![ParameterSpec {
                    name: "x".to_string(),
                    has_default: false,
                    variadic: false,
                }],
                return_rule: ReturnRule::Fixed("integer".to_string()),
                ..Default::default()
            },
        );
        let sem = cat.lookup("base::length").unwrap();
        assert_eq!(sem.parameters.len(), 1);
        assert_eq!(sem.return_rule, ReturnRule::Fixed("integer".to_string()));
    }

    #[test]
    fn catalog_package_functions() {
        let mut cat = InMemoryCatalog::new();
        cat.register("base::length", FunctionSemantics::default());
        cat.register("base::nrow", FunctionSemantics::default());
        cat.register("dplyr::mutate", FunctionSemantics::default());
        assert_eq!(cat.package_functions("base").len(), 2);
        assert_eq!(cat.package_functions("dplyr").len(), 1);
    }

    #[test]
    fn default_semantics_are_eager_unknown() {
        let sem = FunctionSemantics::default();
        assert_eq!(sem.evaluation, Evaluation::Eager);
        assert_eq!(sem.return_rule, ReturnRule::Unknown);
        assert_eq!(sem.flow_effect, FlowEffect::None);
    }
}
