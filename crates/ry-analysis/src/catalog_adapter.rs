//! Adapter that converts ry-typeshed data to the catalog IR.
//!
//! P39-W4: This module bridges the existing vendored typeshed JSON
//! (via `ry_typeshed::FunctionSig`) and the neutral `FunctionSemantics`
//! IR defined in `catalog.rs`. Future workstreams will replace the
//! JSON loader with a compiled catalog pack.

use crate::catalog::{
    BindingEffect, DefusingKind, Evaluation, FlowEffect, FunctionSemantics, ParameterSpec,
    ReturnRule, SelectKind,
};
use ry_typeshed::{EvalMode, FunctionSig};

/// Convert a typeshed `FunctionSig` to the neutral `FunctionSemantics` IR.
pub fn convert_function_sig(sig: &FunctionSig) -> FunctionSemantics {
    FunctionSemantics {
        parameters: sig.params.iter().map(convert_param).collect(),
        evaluation: convert_evaluation(sig),
        return_rule: convert_return_rule(sig),
        flow_effect: convert_flow_effect(sig),
        binding_effect: convert_binding_effect(sig),
        dispatch: None,
    }
}

fn convert_param(p: &ry_typeshed::ParamSpec) -> ParameterSpec {
    ParameterSpec {
        name: p.name.clone(),
        has_default: p.default.is_some(),
        variadic: p.name == "...",
    }
}

fn convert_evaluation(sig: &FunctionSig) -> Evaluation {
    // Check for promise-capture / defusing modes.
    let modes: Vec<&EvalMode> = sig.eval.values().collect();
    if modes.iter().any(|m| matches!(m, EvalMode::QuotedSymbol)) {
        return Evaluation::PromiseCapture {
            defusing: DefusingKind::Ensym,
        };
    }
    if modes
        .iter()
        .any(|m| matches!(m, EvalMode::QuotedExpression))
    {
        return Evaluation::PromiseCapture {
            defusing: DefusingKind::Enquos,
        };
    }
    if modes.iter().any(|m| matches!(m, EvalMode::CapturesPromise)) {
        return Evaluation::PromiseCapture {
            defusing: DefusingKind::Enquo,
        };
    }
    if modes.iter().any(|m| matches!(m, EvalMode::TidySelect)) {
        return Evaluation::DataMask {
            select: Some(SelectKind::ColumnSelect),
        };
    }
    if modes.iter().any(|m| matches!(m, EvalMode::DataMask)) {
        return Evaluation::DataMask { select: None };
    }
    Evaluation::Eager
}

fn convert_return_rule(sig: &FunctionSig) -> ReturnRule {
    if sig.no_return {
        return ReturnRule::Unknown;
    }
    match &sig.return_ {
        ry_typeshed::ReturnSpec::Slot(_) => ReturnRule::Conditional,
        ry_typeshed::ReturnSpec::Concrete(_) => ReturnRule::Fixed("typed".to_string()),
    }
}

fn convert_flow_effect(sig: &FunctionSig) -> FlowEffect {
    if sig.no_return {
        return FlowEffect::NoReturn;
    }
    if sig.predicate.is_some() {
        return FlowEffect::Predicate {
            target: crate::catalog::PredicateTarget::FirstArg,
        };
    }
    if sig.assertion.is_some() {
        return FlowEffect::Assertion { stop_on_fail: true };
    }
    FlowEffect::None
}

fn convert_binding_effect(sig: &FunctionSig) -> Option<BindingEffect> {
    if !sig.injects.is_empty() {
        let names: Vec<String> = sig
            .injects
            .iter()
            .flat_map(|i| i.names.iter().cloned())
            .collect();
        if names.is_empty() {
            return Some(BindingEffect::Inject(vec![]));
        }
        return Some(BindingEffect::Inject(names));
    }
    None
}

/// Build an InMemoryCatalog from a set of loaded typeshed packages.
pub fn catalog_from_typeshed(
    packages: &std::collections::BTreeMap<String, ry_typeshed::Typeshed>,
) -> crate::catalog::InMemoryCatalog {
    let mut catalog = crate::catalog::InMemoryCatalog::new();
    for (pkg_name, typeshed) in packages {
        for (fn_name, sig) in &typeshed.functions {
            let full_name = format!("{}::{}", pkg_name, fn_name);
            catalog.register(&full_name, convert_function_sig(sig));
        }
    }
    catalog
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_basic_sig() -> FunctionSig {
        FunctionSig {
            params: vec![ry_typeshed::ParamSpec {
                name: "x".to_string(),
                type_: None,
                required: false,
                default: None,
            }],
            return_: ry_typeshed::ReturnSpec::Concrete(ry_typeshed::JsonRType {
                mode: "integer".to_string(),
                length: "unknown".to_string(),
                na: false,
                class: vec![],
                columns: std::collections::BTreeMap::new(),
                note: None,
                members: vec![],
            }),
            aliases: vec![],
            eval: Default::default(),
            no_return: false,
            data_mask_source: None,
            schema_effect: None,
            scope_effect: None,
            conditional_scope_effect: None,
            predicate: None,
            assertion: None,
            return_length: None,
            higher_order: None,
            injects: vec![],
            source_relative_path_arg: None,
        }
    }

    #[test]
    fn convert_basic_eager_function() {
        let sig = make_basic_sig();
        let sem = convert_function_sig(&sig);
        assert_eq!(sem.evaluation, Evaluation::Eager);
        assert_eq!(sem.return_rule, ReturnRule::Fixed("typed".to_string()));
        assert_eq!(sem.flow_effect, FlowEffect::None);
    }

    #[test]
    fn convert_promise_capture() {
        let mut sig = make_basic_sig();
        sig.eval
            .insert("expr".to_string(), EvalMode::QuotedExpression);
        let sem = convert_function_sig(&sig);
        assert_eq!(
            sem.evaluation,
            Evaluation::PromiseCapture {
                defusing: DefusingKind::Enquos
            }
        );
    }

    #[test]
    fn convert_no_return() {
        let mut sig = make_basic_sig();
        sig.no_return = true;
        let sem = convert_function_sig(&sig);
        assert_eq!(sem.flow_effect, FlowEffect::NoReturn);
        assert_eq!(sem.return_rule, ReturnRule::Unknown);
    }
}
