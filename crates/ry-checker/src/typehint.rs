//! Static reader for the audited typehint 0.1.0 `#|` convention.
//!
//! Comments are lexical parser tokens, not strings or evaluated R code. The
//! reader only creates records for explicitly scoped, named functions with
//! braced bodies. Unsupported clauses remain attached residuals; they cannot
//! authorize entry assumptions in the declaration checker.

use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::path::Path;

use ry_config::config::ScopedPaths;
use ry_core::Span;
use ry_core::ast::{Expr, SourceFile, Stmt};
use ry_core::declarations::{
    AssignmentSemantics, DeclarationRecord, DeclarationSource, DeclarationTarget,
    DeclaredParameter, DeclaredSignature, EvaluationSemantics, EvidenceUse, ParameterForm,
    ResidualConstraint, SupplyStatus, Translation, TypeExpr,
};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};

const MAX_CLAUSES_PER_FUNCTION: usize = 64;

struct NamedFunction {
    name: String,
    span: Span,
    params: Vec<(String, bool)>,
}

struct Clause<'a> {
    span: Span,
    argument: Option<&'a str>,
    class: Option<&'a str>,
    residual: Option<ResidualConstraint>,
    error: Option<String>,
}

/// Read explicitly adopted comments from a parsed source file. Invalid R
/// input is never evidence for a declaration: its recovered AST may invent a
/// function boundary, so the ordinary parser diagnostic owns that file.
pub fn read_records(file: &SourceFile, scope: &ScopedPaths) -> Vec<DeclarationRecord> {
    if !scope.matches(Path::new(&file.path))
        || !file.parse_errors.is_empty()
        || !file.syntax_violations.is_empty()
        || !file.invalid_utf8.is_empty()
        || file.leading_bom
    {
        return Vec::new();
    }

    let mut named = Vec::new();
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node, _| {
        if let AstNode::Stmt(Stmt::Assign {
            target: Expr::Ident { name, .. },
            value: Expr::Function { params, span, .. },
            ..
        }) = node
        {
            named.push(NamedFunction {
                name: name.clone(),
                span: *span,
                params: params
                    .iter()
                    .map(|param| (param.name.clone(), param.default.is_some()))
                    .collect(),
            });
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });

    let mut line_starts = vec![0];
    for (offset, byte) in file.source.bytes().enumerate() {
        if byte == b'\n' {
            line_starts.push(offset + 1);
        }
    }
    let mut grouped: BTreeMap<(usize, usize), Vec<Clause<'_>>> = BTreeMap::new();
    for comment in &file.comments {
        let Some(start) = line_starts
            .get(comment.line)
            .and_then(|line| line.checked_add(comment.col))
        else {
            continue;
        };
        let end = start.saturating_add(1 + comment.body.len());
        let Some(raw) = file.source.get(start..end) else {
            continue;
        };
        if !raw.starts_with('#') || &raw[1..] != comment.body {
            continue;
        }
        let body = comment.body.trim_start();
        let Some(after_pipe) = body.strip_prefix('|') else {
            continue;
        };
        // Quarto cell options use `#| key: value`, not typehint's
        // whitespace-separated `#| formal class` grammar.
        if after_pipe
            .split_whitespace()
            .next()
            .is_some_and(|word| word.ends_with(':'))
        {
            continue;
        }
        let span = Span::new(start, end, comment.line, comment.col);
        let Some(innermost) = file
            .function_bodies
            .iter()
            .filter(|entry| entry.body.start < start && end < entry.body.end)
            .min_by_key(|entry| entry.body.end - entry.body.start)
        else {
            continue;
        };
        if !named.iter().any(|entry| entry.span == innermost.function) {
            continue;
        }
        grouped
            .entry((innermost.function.start, innermost.function.end))
            .or_default()
            .push(parse_clause(after_pipe, span, raw));
    }

    grouped
        .into_iter()
        .filter_map(|(key, clauses)| {
            let target = named
                .iter()
                .find(|entry| (entry.span.start, entry.span.end) == key)?;
            Some(build_record(file, target, clauses))
        })
        .collect()
}

fn parse_clause<'a>(text: &'a str, span: Span, raw: &'a str) -> Clause<'a> {
    let text = text.trim();
    let mut tokens = text.split_whitespace();
    let argument = tokens.next();
    let class = tokens.next();
    let mut clause = Clause {
        span,
        argument,
        class,
        residual: None,
        error: None,
    };
    match (argument, class) {
        (Some(argument), Some(class)) if valid_formal(argument) && valid_class(class) => {}
        _ => {
            clause.error = Some("expected `#| formal class` with simple class spelling".into());
            return clause;
        }
    }
    let rest = text
        .strip_prefix(argument.expect("validated formal"))
        .map(str::trim_start)
        .and_then(|tail| tail.strip_prefix(class.expect("validated class")))
        .map(str::trim)
        .unwrap_or("");
    if !rest.is_empty() {
        // Preserve the exact unsupported expression and its byte span. The
        // upstream provider evaluates dim/not expressions at runtime; ry
        // neither evaluates them nor silently drops them from an Exact record.
        if let Some(relative) = raw.find(&rest) {
            let start = span.start + relative;
            clause.residual = Some(ResidualConstraint {
                raw: rest.into(),
                span: Span::new(start, start + rest.len(), span.line, span.col + relative),
                reason:
                    "typehint dimension, exclusion, or value expression requires runtime evaluation"
                        .into(),
            });
        } else {
            clause.error = Some("cannot locate clause expression in source".into());
        }
    }
    clause
}

fn valid_formal(word: &str) -> bool {
    word != "..."
        && !word.is_empty()
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_'))
}

fn valid_class(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 256
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_'))
}

fn build_record(
    file: &SourceFile,
    target: &NamedFunction,
    clauses: Vec<Clause<'_>>,
) -> DeclarationRecord {
    let first = clauses.first().expect("group is nonempty").span;
    let last = clauses.last().expect("group is nonempty").span;
    let source_span = Span::new(first.start, last.end, first.line, first.col);
    let mut parameters = Vec::new();
    let mut residuals = Vec::new();
    let mut invalid = None;
    if clauses.len() > MAX_CLAUSES_PER_FUNCTION {
        invalid = Some(format!(
            "more than {MAX_CLAUSES_PER_FUNCTION} typehint clauses for one function"
        ));
    }
    for clause in clauses {
        if let Some(error) = clause.error {
            invalid.get_or_insert(error);
            continue;
        }
        let Some(argument) = clause.argument else {
            continue;
        };
        let Some(class) = clause.class else {
            continue;
        };
        let Some((_, defaulted)) = target.params.iter().find(|formal| formal.0 == argument) else {
            invalid.get_or_insert(format!("`{argument}` is not a formal of `{}`", target.name));
            continue;
        };
        if parameters
            .iter()
            .any(|parameter: &DeclaredParameter| parameter.name == argument)
        {
            invalid.get_or_insert(format!("duplicate typehint clause for `{argument}`"));
            continue;
        }
        parameters.push(DeclaredParameter {
            name: argument.into(),
            form: ParameterForm::Ordinary,
            supplied: if *defaulted {
                SupplyStatus::DefaultedSuppliedOnly
            } else {
                SupplyStatus::Required
            },
            evaluation: EvaluationSemantics::Promise,
            constraint: Some(TypeExpr::ExactClass(class.into())),
        });
        if let Some(residual) = clause.residual {
            residuals.push(residual);
        }
    }
    parameters.sort_by_key(|parameter| {
        target
            .params
            .iter()
            .position(|formal| formal.0 == parameter.name)
            .unwrap_or(usize::MAX)
    });
    let signature = DeclaredSignature {
        parameters,
        return_constraint: None,
        assignment: AssignmentSemantics::EntryOnly,
    };
    let translation = if let Some(reason) = invalid {
        Translation::InvalidSyntax(reason)
    } else if let Err(error) = signature.validate() {
        Translation::InvalidSyntax(error.to_string())
    } else if residuals.is_empty() {
        Translation::Exact(signature)
    } else {
        Translation::Partial {
            supported: signature,
            residuals,
        }
    };
    DeclarationRecord {
        source: DeclarationSource {
            provider: "typehint".into(),
            provider_version: Some("0.1.0".into()),
            path: file.path.clone(),
            span: source_span,
            raw: file.source[source_span.start..source_span.end].into(),
            target: DeclarationTarget::LocalFunction {
                path: file.path.clone(),
                definition: target.span,
                display_name: Some(target.name.clone()),
            },
        },
        translation,
        evidence: EvidenceUse::AdoptedContract,
        assumptions: vec![
            "The authored comment is adopted as a static contract; execution of check_types() is not inferred".into(),
            "Simple class clauses compare R's effective class(), not storage typeof()".into(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::RParser;
    use tempfile::tempdir;

    fn read(source: &str) -> Vec<DeclarationRecord> {
        let root = tempdir().unwrap();
        let path = root.path().join("R/main.R");
        let file = RParser::new()
            .unwrap()
            .parse(path.to_str().unwrap(), source)
            .unwrap();
        let scope = ScopedPaths::new(root.path(), &["R/**".into()]).unwrap();
        read_records(&file, &scope)
    }

    #[test]
    fn reads_late_body_comments_and_innermost_named_function_only() {
        let records = read(
            "f <- function(x) {\n  x\n  #| x integer\n  inner <- function(y) {\n    #| y character\n    y\n  }\n  lapply(list(1L), function(z) { #| z logical\n    z\n  })\n  inner(x)\n}\n",
        );
        assert_eq!(records.len(), 2, "{records:?}");
        for (record, name) in records.iter().zip(["f", "inner"]) {
            assert!(matches!(record.translation, Translation::Exact(_)));
            assert!(matches!(
                &record.source.target,
                DeclarationTarget::LocalFunction { display_name: Some(actual), .. } if actual == name
            ));
        }
    }

    #[test]
    fn ignores_quarto_options_strings_header_and_ordinary_comments() {
        let records = read(
            "f <- function(\n  x = { #| x integer\n    1L\n  }\n) {\n  text <- \"#| x character\"\n  #| label: sample\n  # ordinary comment\n  #| x integer\n}\n",
        );
        assert_eq!(records.len(), 1, "{records:?}");
        assert!(matches!(records[0].translation, Translation::Exact(_)));
        assert_eq!(records[0].source.raw, "#| x integer");
    }

    #[test]
    fn preserves_unsupported_residual_and_crlf_bytes() {
        let source = "f <- function(x) {\r\n  #| x integer dim(>=2,  4) not(NA)\r\n  x\r\n}\r\n";
        let records = read(source);
        assert_eq!(records.len(), 1);
        let Translation::Partial { residuals, .. } = &records[0].translation else {
            panic!("expected residual: {records:?}");
        };
        assert_eq!(residuals.len(), 1);
        assert_eq!(
            &source[residuals[0].span.start..residuals[0].span.end],
            "dim(>=2,  4) not(NA)"
        );
    }

    #[test]
    fn defaults_are_supplied_only_and_bad_clauses_are_explicit() {
        let records = read("f <- function(x = 1L) {\n #| x integer\n}\n");
        let Translation::Exact(signature) = &records[0].translation else {
            panic!("expected exact");
        };
        assert_eq!(
            signature.parameters[0].supplied,
            SupplyStatus::DefaultedSuppliedOnly
        );
        assert_eq!(
            DeclaredSignature::parse(&signature.canonical().unwrap())
                .unwrap()
                .parameters[0]
                .supplied,
            SupplyStatus::DefaultedSuppliedOnly
        );
        let invalid = read("f <- function(x) {\n #| x\n}\n");
        assert!(matches!(
            invalid[0].translation,
            Translation::InvalidSyntax(_)
        ));
    }
}
