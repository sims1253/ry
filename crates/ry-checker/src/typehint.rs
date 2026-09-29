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

use crate::infer::semantic_argument_name;

const MAX_CLAUSES_PER_FUNCTION: usize = 64;
const MAX_ANNOTATION_BYTES: usize = 4096;

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
    read_records_at(file, Path::new(&file.path), scope)
}

/// Match adoption against the native input path before the parser's
/// displayable path can lose filename bytes. The record itself retains the
/// parser path because checker and export source coordinates use that text.
pub fn read_records_at(
    file: &SourceFile,
    native_path: &Path,
    scope: &ScopedPaths,
) -> Vec<DeclarationRecord> {
    if !scope.matches(native_path)
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
                    .map(|param| {
                        (
                            semantic_argument_name(&param.name).to_owned(),
                            param.default.is_some(),
                        )
                    })
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
        let full_end = start.saturating_add(1 + comment.body.len());
        let mut end = full_end.min(start.saturating_add(MAX_ANNOTATION_BYTES));
        while end > start && !file.source.is_char_boundary(end) {
            end -= 1;
        }
        let Some(full_raw) = file.source.get(start..full_end) else {
            continue;
        };
        if !full_raw.starts_with('#') || full_raw[1..] != comment.body {
            continue;
        }
        let raw = &full_raw[..end - start];
        // The audited provider requires '#' and '|' to be adjacent. A
        // regular '# | ...' comment is not a typehint declaration.
        let Some(after_pipe) = comment.body.strip_prefix('|') else {
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
        let clauses = grouped
            .entry((innermost.function.start, innermost.function.end))
            .or_default();
        // The extra entry records overflow without retaining an unbounded
        // number of annotations from a generated or hostile source file.
        if clauses.len() <= MAX_CLAUSES_PER_FUNCTION {
            let clause = if full_end > end {
                Clause {
                    span,
                    argument: None,
                    class: None,
                    residual: None,
                    error: Some(format!(
                        "typehint clause exceeds {MAX_ANNOTATION_BYTES} byte reader budget"
                    )),
                }
            } else {
                parse_clause(after_pipe, span, raw)
            };
            clauses.push(clause);
        }
    }

    grouped
        .into_iter()
        .flat_map(|(key, clauses)| {
            let Some(target) = named
                .iter()
                .find(|entry| (entry.span.start, entry.span.end) == key)
            else {
                return Vec::new();
            };
            let mut claims = BTreeMap::new();
            let conflicting = clauses.iter().any(|clause| {
                let Some(argument) = clause.argument else {
                    return false;
                };
                let claim = (
                    clause.class,
                    clause
                        .residual
                        .as_ref()
                        .map(|residual| residual.raw.as_str()),
                );
                claims
                    .insert(argument, claim)
                    .is_some_and(|old| old != claim)
            });
            let within_reader_budget = clauses.len() <= MAX_CLAUSES_PER_FUNCTION
                && clauses
                    .last()
                    .and_then(|last| {
                        clauses
                            .first()
                            .map(|first| last.span.end.saturating_sub(first.span.start))
                    })
                    .is_some_and(|bytes| bytes <= MAX_ANNOTATION_BYTES);
            if conflicting && within_reader_budget {
                // Each comment is a faithful claim in its own source span.
                // Selecting one combined signature would silently choose a
                // side; the shared resolver sees distinct exact signatures
                // and emits an explicit conflict instead.
                clauses
                    .into_iter()
                    .map(|clause| build_record(file, target, vec![clause]))
                    .collect()
            } else {
                vec![build_record(file, target, clauses)]
            }
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
        (Some(argument), Some(_)) if valid_formal(argument) => {}
        _ => {
            clause.error = Some("expected `#| formal class`".into());
            return clause;
        }
    }
    if !valid_class(class.expect("validated class token")) {
        let class = class.expect("validated class token");
        if let Some(relative) = raw.find(class) {
            let start = span.start + relative;
            clause.residual = Some(ResidualConstraint {
                raw: raw[relative..].into(),
                span: Span::new(start, span.end, span.line, span.col + relative),
                reason: "class spelling is outside the audited simple typehint subset".into(),
            });
        } else {
            clause.error = Some("cannot locate unsupported class in source".into());
        }
        return clause;
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
        if let Some(relative) = raw.find(rest) {
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
    let full_span = Span::new(first.start, last.end, first.line, first.col);
    let source_span = if full_span.end.saturating_sub(full_span.start) > MAX_ANNOTATION_BYTES {
        first
    } else {
        full_span
    };
    let mut parameters = Vec::new();
    let mut residuals = Vec::new();
    let mut invalid = None;
    let mut ambiguous = None;
    if clauses.len() > MAX_CLAUSES_PER_FUNCTION {
        invalid = Some(format!(
            "more than {MAX_CLAUSES_PER_FUNCTION} typehint clauses for one function"
        ));
    }
    if full_span.end.saturating_sub(full_span.start) > MAX_ANNOTATION_BYTES {
        invalid = Some(format!(
            "typehint annotation range exceeds {MAX_ANNOTATION_BYTES} byte reader budget"
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
            if target.params.iter().any(|formal| formal.0.contains('\\')) {
                ambiguous.get_or_insert(
                    "an encoded or escaped R formal cannot be matched by the static typehint reader"
                        .to_owned(),
                );
            } else {
                invalid.get_or_insert(format!("`{argument}` is not a formal of `{}`", target.name));
            }
            continue;
        };
        if let Some(residual) = clause.residual {
            residuals.push(residual);
        }
        if !valid_class(class) {
            continue;
        }
        if parameters
            .iter()
            .any(|parameter: &DeclaredParameter| parameter.name == argument)
        {
            // Repeated identical clauses do not add a new claim.
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
    } else if let Some(reason) = ambiguous {
        Translation::AmbiguousAttachment(reason)
    } else if let Err(error) = signature.validate() {
        Translation::InvalidSyntax(error.to_string())
    } else if residuals.is_empty() {
        Translation::Exact(signature)
    } else if signature.parameters.is_empty() {
        Translation::Unsupported { residuals }
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
    use crate::{DeclarationFindingKind, Project};
    use ry_core::RParser;
    use std::sync::Arc;
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
            "f <- function(\n  x = { #| x integer\n    1L\n  }\n) {\n  text <- \"#| x character\"\n  #| label: sample\n  # | x character\n  # ordinary comment\n  #| x integer\n}\n",
        );
        assert_eq!(records.len(), 1, "{records:?}");
        assert!(matches!(records[0].translation, Translation::Exact(_)));
        assert_eq!(records[0].source.raw, "#| x integer");
    }

    #[test]
    fn parser_recovery_is_not_a_source_of_contracts() {
        let root = tempdir().unwrap();
        let path = root.path().join("R/main.R");
        let source = "f <- function(x) {\n #| x integer\n x\n}\ny <- (\n";
        let file = RParser::new()
            .unwrap()
            .parse(path.to_str().unwrap(), source)
            .unwrap();
        assert!(!file.parse_errors.is_empty());
        let scope = ScopedPaths::new(root.path(), &["R/**".into()]).unwrap();
        assert!(read_records(&file, &scope).is_empty());
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

    #[test]
    fn plain_backtick_formals_attach_by_semantic_name_but_encoded_names_decline() {
        let quoted = read("f <- function(`x`) {\n #| x integer\n `x`\n}\n");
        assert!(matches!(quoted[0].translation, Translation::Exact(_)));
        let non_syntactic = read("f <- function(`a b`) {\n #| a b integer\n `a b`\n}\n");
        assert!(matches!(
            non_syntactic[0].translation,
            Translation::InvalidSyntax(_)
        ));
        let escaped = read("f <- function(`\\x78`) {\n #| x integer\n `\\x78`\n}\n");
        assert!(matches!(
            escaped[0].translation,
            Translation::AmbiguousAttachment(_)
        ));
    }

    #[test]
    fn unsupported_class_spelling_remains_a_source_residual() {
        let source = "f <- function(x) {\n #| x some-class\n x\n}\n";
        let records = read(source);
        let Translation::Unsupported { residuals } = &records[0].translation else {
            panic!("unsupported class must stay visible: {records:?}");
        };
        assert_eq!(residuals.len(), 1);
        assert_eq!(residuals[0].raw, "some-class");
        assert_eq!(
            &source[residuals[0].span.start..residuals[0].span.end],
            residuals[0].raw
        );
    }

    #[test]
    fn contradictory_comments_remain_distinct_records_and_identical_repeats_do_not_conflict() {
        let records = read("f <- function(x) {\n #| x integer\n #| x character\n x\n}\n");
        assert_eq!(records.len(), 2);
        assert!(
            records
                .iter()
                .all(|record| matches!(record.translation, Translation::Exact(_)))
        );
        assert_eq!(records[0].source.raw, "#| x integer");
        assert_eq!(records[1].source.raw, "#| x character");

        let repeated = read("f <- function(x) {\n #| x integer\n #| x integer\n x\n}\n");
        assert_eq!(repeated.len(), 1);
        assert!(matches!(repeated[0].translation, Translation::Exact(_)));
    }

    #[test]
    fn reader_limits_clause_bytes_range_and_count() {
        let long_class = "a".repeat(MAX_ANNOTATION_BYTES + 1);
        let records = read(&format!("f <- function(x) {{\n #| x {long_class}\n}}\n"));
        assert!(matches!(
            records[0].translation,
            Translation::InvalidSyntax(_)
        ));
        assert!(records[0].source.raw.len() <= MAX_ANNOTATION_BYTES);

        let separated = format!(
            "f <- function(x) {{\n #| x integer\n {}#| x integer\n}}\n",
            " ".repeat(MAX_ANNOTATION_BYTES)
        );
        let records = read(&separated);
        assert!(matches!(
            records[0].translation,
            Translation::InvalidSyntax(_)
        ));
        assert!(records[0].source.raw.len() <= MAX_ANNOTATION_BYTES);

        let many = format!(
            "f <- function(x) {{\n{} }}\n",
            " #| x integer\n".repeat(MAX_CLAUSES_PER_FUNCTION + 20)
        );
        let records = read(&many);
        assert!(matches!(
            records[0].translation,
            Translation::InvalidSyntax(_)
        ));
        assert!(records[0].source.raw.len() <= MAX_ANNOTATION_BYTES);

        let alternating = format!(
            "f <- function(x) {{\n{} }}\n",
            (0..MAX_CLAUSES_PER_FUNCTION + 20)
                .map(|index| if index % 2 == 0 {
                    " #| x integer\n"
                } else {
                    " #| x character\n"
                })
                .collect::<String>()
        );
        let records = read(&alternating);
        assert_eq!(records.len(), 1);
        assert!(matches!(
            records[0].translation,
            Translation::InvalidSyntax(_)
        ));
    }

    #[test]
    fn short_comment_fuzz_keeps_spans_and_statuses_bounded() {
        let root = tempdir().unwrap();
        let path = root.path().join("R/fuzz.R");
        let scope = ScopedPaths::new(root.path(), &["R/**".into()]).unwrap();
        let mut parser = RParser::new().unwrap();
        let alphabet = [b'a', b'0', b' ', b'|', b'#', b'(', b')', b'_', b':', b'-'];
        let mut state = 0x3141_5926_u32;
        for _ in 0..512 {
            let mut payload = String::new();
            for _ in 0..32 {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                payload.push(alphabet[(state as usize) % alphabet.len()] as char);
            }
            let source = format!("f <- function(x) {{\n #| {payload}\n x\n}}\n");
            let file = parser.parse(path.to_str().unwrap(), &source).unwrap();
            let records = read_records(&file, &scope);
            assert!(records.len() <= 1);
            for record in records {
                assert_eq!(
                    source.get(record.source.span.start..record.source.span.end),
                    Some(record.source.raw.as_str())
                );
                assert!(record.source.raw.len() <= MAX_ANNOTATION_BYTES);
                if let Translation::Exact(signature)
                | Translation::Partial {
                    supported: signature,
                    ..
                } = &record.translation
                {
                    assert!(signature.canonical().is_ok());
                }
            }
        }
    }

    #[test]
    fn annotation_only_warm_edits_retract_and_restore_provider_findings() {
        let root = tempdir().unwrap();
        let path = root.path().join("R/main.R");
        let scope = ScopedPaths::new(root.path(), &["R/**".into()]).unwrap();
        let mut parser = RParser::new().unwrap();
        let source =
            |comment: &str| format!("f <- function(x) {{\n {comment}\n x\n}}\nf(\"bad\")\n");
        let original = parser
            .parse(path.to_str().unwrap(), &source("#| x integer"))
            .unwrap();
        let mut warm = Project::new();
        warm.add_file(original.path.clone(), original.clone());
        warm.set_declaration_records(read_records(&original, &scope));
        warm.check_incremental();
        assert_eq!(
            warm.declaration_findings()
                .iter()
                .flat_map(|(_, findings)| findings)
                .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
                .count(),
            1
        );

        for (comment, expected) in [
            ("#| x character", 0),
            ("# ordinary comment", 0),
            ("#| x integer", 1),
        ] {
            let edited = parser
                .parse(path.to_str().unwrap(), &source(comment))
                .unwrap();
            let records = read_records(&edited, &scope);
            warm.update_file(edited.path.clone(), Arc::new(edited.clone()));
            warm.set_declaration_records(records.clone());
            warm.check_incremental();
            let findings = warm
                .declaration_findings()
                .iter()
                .flat_map(|(_, findings)| findings)
                .filter(|finding| finding.kind == DeclarationFindingKind::Mismatch)
                .count();
            assert_eq!(findings, expected, "{comment}");

            let mut cold = Project::new();
            cold.add_file(edited.path.clone(), edited);
            cold.set_declaration_records(records);
            cold.check();
            assert_eq!(warm.declaration_findings(), cold.declaration_findings());
        }
    }
}
