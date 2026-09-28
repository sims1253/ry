//! Annotation-record serializer for the existing `dump-facts` export.
//! The public schema selector is enabled in #594, with real source records.

use miette::{Result, miette};
use ry_core::SourceFile;
use ry_core::declarations::{
    AssignmentSemantics, DeclarationRecord, DeclarationTarget, DeclaredParameter,
    DeclaredSignature, EvaluationSemantics, EvidenceUse, ParameterForm, ResidualConstraint,
    SupplyStatus, Translation, TypeExpr,
};
use serde_json::{Value, json};

use crate::facts::source_span;

fn constraint(ty: &TypeExpr) -> Result<String> {
    ty.canonical()
        .map_err(|error| miette!("cannot export declaration: {error}"))
}

fn signature(signature: &DeclaredSignature) -> Result<Value> {
    signature
        .canonical()
        .map_err(|error| miette!("cannot export declaration: {error}"))?;
    let parameters = signature
        .parameters
        .iter()
        .map(parameter)
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({
        "parameters": parameters,
        "return_constraint": signature.return_constraint.as_ref().map(constraint).transpose()?,
        "assignment_semantics": match signature.assignment {
            AssignmentSemantics::EntryOnly => "entry_only",
            AssignmentSemantics::PersistentBinding => "persistent_binding",
            AssignmentSemantics::CoercesInput => "coerces_input",
            AssignmentSemantics::Unknown => "unknown",
        },
    }))
}

fn parameter(parameter: &DeclaredParameter) -> Result<Value> {
    Ok(json!({
        "name": parameter.name,
        "form": match parameter.form {
            ParameterForm::Ordinary => "ordinary",
            ParameterForm::Variadic => "variadic",
        },
        "supplied": match parameter.supplied {
            SupplyStatus::Required => "required",
            SupplyStatus::Defaulted => "defaulted",
            SupplyStatus::Unknown => "unknown",
        },
        "evaluation": match parameter.evaluation {
            EvaluationSemantics::Value => "value",
            EvaluationSemantics::Promise => "promise",
            EvaluationSemantics::Quoted => "quoted",
            EvaluationSemantics::Unknown => "unknown",
        },
        "constraint": parameter.constraint.as_ref().map(constraint).transpose()?,
    }))
}

fn residual(
    residual: &ResidualConstraint,
    file: &SourceFile,
    source_matches: bool,
    source_span_range: ry_core::Span,
) -> Result<Value> {
    if source_matches
        && (residual.span.start < source_span_range.start
            || residual.span.end > source_span_range.end
            || residual.span.start > residual.span.end
            || file.source.get(residual.span.start..residual.span.end) != Some(&residual.raw))
    {
        return Err(miette!("stale annotation residual in {}", file.path));
    }
    Ok(json!({
        "raw": residual.raw,
        "span": if source_matches { source_span(&file.source, residual.span) } else { Value::Null },
        "reason": residual.reason,
    }))
}

/// Export records attached to this file. External-provider source spans stay
/// unavailable unless the exact source text is in this file; a same-spelled
/// target name never fabricates a definition location.
pub(crate) fn export_records(file: &SourceFile, records: &[DeclarationRecord]) -> Result<Value> {
    let mut output = Vec::with_capacity(records.len());
    for record in records {
        let source_matches = record.source.path == file.path;
        if source_matches {
            let actual = file
                .source
                .get(record.source.span.start..record.source.span.end)
                .ok_or_else(|| miette!("stale annotation source span in {}", file.path))?;
            if actual != record.source.raw {
                return Err(miette!("stale annotation source text in {}", file.path));
            }
        }
        let target = match &record.source.target {
            DeclarationTarget::LocalFunction {
                path,
                definition,
                display_name,
            } => json!({
                "kind": "local_function",
                "path": path,
                "definition_span": if path == &file.path { source_span(&file.source, *definition) } else { Value::Null },
                "display_name": display_name,
            }),
            DeclarationTarget::PackageFunction { package, name } => json!({
                "kind": "package_function", "package": package, "name": name,
            }),
        };
        let translation = match &record.translation {
            Translation::Exact(supported) => json!({
                "kind": "exact", "supported": signature(supported)?, "residuals": [],
            }),
            Translation::Partial {
                supported,
                residuals,
            } => {
                if residuals.is_empty() {
                    return Err(miette!("partial annotation lacks unsupported residual"));
                }
                json!({
                    "kind": "partial", "supported": signature(supported)?,
                    "residuals": residuals.iter().map(|item| residual(item, file, source_matches, record.source.span)).collect::<Result<Vec<_>>>()?,
                })
            }
            Translation::Unsupported { residuals } => {
                if residuals.is_empty() {
                    return Err(miette!("unsupported annotation lacks residual"));
                }
                json!({
                    "kind": "unsupported", "supported": Value::Null,
                    "residuals": residuals.iter().map(|item| residual(item, file, source_matches, record.source.span)).collect::<Result<Vec<_>>>()?,
                })
            }
            Translation::InvalidSyntax(reason) => json!({
                "kind": "invalid_syntax", "supported": Value::Null,
                "reason": reason,
            }),
            Translation::AmbiguousAttachment(reason) => json!({
                "kind": "ambiguous_attachment", "supported": Value::Null,
                "reason": reason,
            }),
        };
        output.push(json!({
            "source": {
                "provider": record.source.provider,
                "provider_version": record.source.provider_version,
                "path": record.source.path,
                "span": if source_matches { source_span(&file.source, record.source.span) } else { Value::Null },
                "raw": record.source.raw,
            },
            "target": target,
            "translation": translation,
            "evidence_use": match record.evidence {
                EvidenceUse::AdoptedContract => "adopted_contract",
                EvidenceUse::RuntimeGuard => "runtime_guard",
                EvidenceUse::DocumentationCandidate => "documentation_candidate",
            },
            "assumptions": record.assumptions,
        }));
    }
    output.sort_by_cached_key(Value::to_string);
    Ok(json!(output))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_core::RParser;
    use ry_core::Span;
    use ry_core::declarations::{
        AtomicMode, DeclarationSource, DeclaredLength, EvaluationSemantics,
    };

    #[test]
    fn annotations_preserve_separate_translation_and_evidence_status() {
        let src = "f <- function(x) {\n  #| x integer not(NA)\n  x\n}\n";
        let mut parser = RParser::new().unwrap();
        let file = parser.parse("test.R", src).unwrap();
        let raw = "#| x integer not(NA)";
        let start = src.find(raw).unwrap();
        let record = DeclarationRecord {
            source: DeclarationSource {
                provider: "typehint".into(),
                provider_version: Some("pinned-test".into()),
                path: "test.R".into(),
                span: Span::new(start, start + raw.len(), 1, 2),
                raw: raw.into(),
                target: DeclarationTarget::LocalFunction {
                    path: "test.R".into(),
                    definition: Span::new(5, src.rfind('}').unwrap() + 1, 0, 5),
                    display_name: Some("f".into()),
                },
            },
            translation: Translation::Partial {
                supported: DeclaredSignature {
                    parameters: vec![DeclaredParameter {
                        name: "x".into(),
                        form: ParameterForm::Ordinary,
                        supplied: SupplyStatus::Required,
                        evaluation: EvaluationSemantics::Promise,
                        constraint: Some(TypeExpr::Atomic {
                            mode: AtomicMode::Integer,
                            length: Some(DeclaredLength::Exact(1)),
                        }),
                    }],
                    return_constraint: None,
                    assignment: AssignmentSemantics::EntryOnly,
                },
                residuals: vec![ResidualConstraint {
                    raw: "not(NA)".into(),
                    span: Span::new(start + 13, start + raw.len(), 1, 15),
                    reason: "value exclusion is unsupported".into(),
                }],
            },
            evidence: EvidenceUse::DocumentationCandidate,
            assumptions: vec!["boolean check is not a successful runtime guard".into()],
        };
        let value = export_records(&file, std::slice::from_ref(&record)).unwrap();
        let entry = &value[0];
        assert_eq!(entry["translation"]["kind"], "partial");
        assert_eq!(
            entry["translation"]["supported"]["parameters"][0]["constraint"],
            "integer<len=1>"
        );
        assert_eq!(entry["translation"]["residuals"][0]["raw"], "not(NA)");
        assert_eq!(entry["evidence_use"], "documentation_candidate");
        assert_eq!(
            entry["source"]["span"]["bytes"],
            json!([start, start + raw.len()])
        );
        assert_eq!(entry["target"]["kind"], "local_function");

        let mut exact = record.clone();
        let Translation::Partial { supported, .. } = &record.translation else {
            unreachable!();
        };
        exact.translation = Translation::Exact(supported.clone());
        exact.evidence = EvidenceUse::AdoptedContract;
        let exact_value = export_records(&file, &[exact]).unwrap();
        assert_eq!(exact_value[0]["translation"]["kind"], "exact");
        assert_eq!(exact_value[0]["translation"]["residuals"], json!([]));
        assert_eq!(exact_value[0]["evidence_use"], "adopted_contract");

        let mut stale = record.clone();
        if let Translation::Partial { residuals, .. } = &mut stale.translation {
            residuals[0].span.start += 1;
        }
        assert!(export_records(&file, &[stale]).is_err());

        let mut external = record.clone();
        external.source.path = "upstream.R".into();
        if let DeclarationTarget::LocalFunction { path, .. } = &mut external.source.target {
            *path = "upstream.R".into();
        }
        let external_value = export_records(&file, &[external]).unwrap();
        assert!(external_value[0]["source"]["span"].is_null());
        assert!(external_value[0]["target"]["definition_span"].is_null());
        assert!(external_value[0]["translation"]["residuals"][0]["span"].is_null());

        let mut unsupported = record.clone();
        unsupported.translation = Translation::Unsupported {
            residuals: vec![ResidualConstraint {
                raw: raw.into(),
                span: record.source.span,
                reason: "provider predicate is not supported".into(),
            }],
        };
        let unsupported_value = export_records(&file, &[unsupported]).unwrap();
        assert_eq!(unsupported_value[0]["translation"]["kind"], "unsupported");
        assert!(unsupported_value[0]["translation"]["supported"].is_null());

        let mut invalid = record.clone();
        invalid.translation = Translation::InvalidSyntax("expected one predicate".into());
        assert_eq!(
            export_records(&file, &[invalid]).unwrap()[0]["translation"]["kind"],
            "invalid_syntax"
        );
        let mut ambiguous = record;
        ambiguous.translation = Translation::AmbiguousAttachment("two local definitions".into());
        assert_eq!(
            export_records(&file, &[ambiguous]).unwrap()[0]["translation"]["kind"],
            "ambiguous_attachment"
        );
    }
}
