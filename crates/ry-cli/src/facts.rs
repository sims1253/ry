//! Versioned scope-exit evidence. This module exports the checker's snapshots;
//! it never reconstructs flow state or resolves references by spelling.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use miette::{IntoDiagnostic, Result};
use ry_checker::{ScopeRecord, ScopeRecordKind};
use ry_core::{SourceFile, Span};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{check, dump, facts_types, pipeline};

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn executable_digest() -> Result<String> {
    let mut file =
        std::fs::File::open(std::env::current_exe().into_diagnostic()?).into_diagnostic()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer).into_diagnostic()?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn json_digest(value: &Value) -> String {
    digest(&serde_json::to_vec(value).expect("JSON values serialize"))
}

fn utf8_path(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| miette::miette!("dump-facts requires UTF-8 paths: {}", path.display()))
}

fn file_identity(path: &Path) -> Result<String> {
    let path = path.canonicalize().into_diagnostic()?;
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| miette::miette!("dump-facts requires UTF-8 file paths: {}", path.display()))
}

fn sorted_sets<K: Ord + Clone + serde::Serialize, V: Ord + Clone + serde::Serialize>(
    map: &HashMap<K, std::collections::HashSet<V>>,
) -> Value {
    let sorted: BTreeMap<_, BTreeSet<_>> = map
        .iter()
        .map(|(key, values)| (key.clone(), values.iter().cloned().collect()))
        .collect();
    json!(sorted)
}

/// Hash exactly the environment supplied to Project, including the static
/// installed-package and serialized-data inventories the resolver consumed.
fn workspace_value(workspace: &ry_workspace::WorkspaceContext) -> Value {
    let loads: BTreeMap<_, _> = workspace
        .load_bindings
        .iter()
        .map(|(path, values)| (path, sorted_sets(values)))
        .collect();
    json!({
        "attached_packages": workspace.attached_packages.iter().collect::<BTreeSet<_>>(),
        "bare_bindings": sorted_sets(&workspace.bare_bindings),
        "external_bindings": sorted_sets(&workspace.external_bindings),
        "imported_bindings": workspace.imported_bindings,
        "s3_methods": sorted_sets(&workspace.s3_methods),
        "load_bindings": loads,
    })
}

fn custom_typeshed_value(stubs: &BTreeMap<String, ry_typeshed::Typeshed>) -> Value {
    let mut packages = BTreeMap::new();
    for (name, stub) in stubs {
        // JSON object keys cannot encode the (generic, class) pair.
        let mut serializable = stub.clone();
        serializable.s3_methods.clear();
        let mut value = json!(serializable);
        value["s3_methods"] = json!(
            stub.s3_methods
                .iter()
                .map(|((generic, class), signature)| {
                    json!({"generic": generic, "class": class, "signature": signature})
                })
                .collect::<Vec<_>>()
        );
        packages.insert(name, value);
    }
    json!(packages)
}

fn source_span(source: &str, span: Span) -> Value {
    if span.start > span.end
        || span.end > source.len()
        || !source.is_char_boundary(span.start)
        || !source.is_char_boundary(span.end)
    {
        return Value::Null;
    }
    let position = |offset| {
        let before = &source[..offset];
        let line_start = before.rfind('\n').map_or(0, |index| index + 1);
        [
            before.bytes().filter(|byte| *byte == b'\n').count() + 1,
            source[line_start..offset].chars().count() + 1,
        ]
    };
    json!({
        "bytes": [span.start, span.end],
        "start": position(span.start),
        "end": position(span.end),
    })
}

fn scope_kind(kind: ScopeRecordKind) -> &'static str {
    match kind {
        ScopeRecordKind::Top => "top",
        ScopeRecordKind::Function => "function",
    }
}

fn export_scopes(file: &SourceFile, mut records: Vec<ScopeRecord>) -> Vec<Value> {
    records.sort_by_key(|record| (record.span.start, record.span.end, scope_kind(record.kind)));
    records.dedup_by(|a, b| a.kind == b.kind && a.span == b.span);
    let mut function_locals = HashMap::new();
    dump::index_scope_bodies(&file.stmts, &mut function_locals);
    let mut top_locals = HashMap::new();
    dump::collect_local_bindings(&file.stmts, &mut top_locals);
    let empty_locals = HashMap::new();
    let mut scopes = Vec::new();
    for record in records {
        let locals = match record.kind {
            ScopeRecordKind::Top => &top_locals,
            ScopeRecordKind::Function => function_locals
                .get(&record.span.start)
                .unwrap_or(&empty_locals),
        };
        let mut bindings = Vec::new();
        for (name, ty) in &record.scope.bindings {
            let parameter = record.params.iter().find(|(param, _)| param == name);
            let (kind, site_kind, site) = if let Some((_, span)) =
                parameter.filter(|_| record.scope.parameter_bindings.contains(name))
            {
                ("param", "formal", Some(*span))
            } else if let Some(span) = locals.get(name) {
                ("local", "first_assignment", Some(*span))
            } else {
                // Scope does not track provenance for every insertion.
                // assign() can shadow inherited or imported names without
                // leaving an AST assignment site in this scope.
                ("unclassified", "unavailable", None)
            };
            let span = site
                .filter(|span| span.start < span.end)
                .map(|span| source_span(&file.source, span))
                .unwrap_or(Value::Null);
            let callee_alias = record
                .scope
                .function_aliases
                .get(name)
                .map(|target| json!({"target": target, "resolution": "not_established"}));
            bindings.push(json!({
                "name": name,
                "kind": kind,
                "snapshot_kind": "scope_exit",
                "type": facts_types::export_type(ty),
                "declaration": {
                    "kind": if span.is_null() { "unavailable" } else { site_kind },
                    "span": span,
                    "defines_final_value": "not_established",
                },
                "origin": {
                    "callee_alias": callee_alias,
                    "list_derived": record.scope.list_origin_bindings.contains(name),
                    "default_parameter_derived": record.scope.default_parameter_bindings.contains(name),
                },
            }));
        }
        bindings.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        scopes.push(json!({
            "kind": scope_kind(record.kind),
            "name": record.name,
            "snapshot_kind": "scope_exit",
            "span": if record.kind == ScopeRecordKind::Top ||
                (record.span.start < record.span.end && function_locals.contains_key(&record.span.start)) {
                source_span(&file.source, record.span)
            } else { Value::Null },
            "data_mask_unknown": record.scope.data_mask_unknown,
            "search_path_unknown": record.scope.search_path_unknown,
            "bindings": bindings,
        }));
    }
    scopes
}

fn export_references(file: &SourceFile, facts: ry_checker::ReferenceFacts) -> (Value, Value) {
    use ry_checker::{ReferenceDefinitionKind, ReferenceResolution};
    let definitions: Vec<_> = facts
        .definitions
        .into_iter()
        .map(|definition| {
            json!({
                "id": definition.id.0,
                "name": definition.name,
                "kind": match definition.kind {
                    ReferenceDefinitionKind::Assignment => "assignment",
                    ReferenceDefinitionKind::Formal => "formal",
                },
                "scope_span": source_span(&file.source, definition.scope_span),
                "span": source_span(&file.source, definition.span),
            })
        })
        .collect();
    let references: Vec<_> = facts.references.into_iter().map(|reference| json!({
        "name": reference.name,
        "span": source_span(&file.source, reference.span),
        "snapshot_kind": "reference",
        "resolution_status": match reference.resolution {
            ReferenceResolution::Resolved => "resolved",
            ReferenceResolution::Ambiguous => "ambiguous",
            ReferenceResolution::Unresolved => "unresolved",
            ReferenceResolution::Unsupported => "unsupported",
        },
        "definition_id": reference.definition.map(|id| id.0),
        "type_at_reference": reference.type_at_reference.as_ref().map(facts_types::export_type),
        "reason": reference.reason,
    })).collect();
    (json!(definitions), json!(references))
}

pub(crate) fn run_dump_facts(
    files: Vec<PathBuf>,
    project_root: Option<PathBuf>,
    format: &str,
    references: bool,
) -> Result<ExitCode> {
    if format != "json" {
        return Err(miette::miette!(
            "unknown --format `{format}`; only `json` is supported"
        ));
    }
    let search_start = files.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let (config_root, cfg) = match pipeline::discover_config(&search_start) {
        Ok(found) => found,
        Err(code) => return Ok(code),
    };
    let mut paths = Vec::new();
    let mut truncations = Vec::new();
    for root in &files {
        utf8_path(root)?;
        if !root.exists() {
            return Err(miette::miette!(
                "{}: no such file or directory",
                root.display()
            ));
        }
        let mut found = ry_workspace::discover_r_files(
            root,
            config_root.as_deref(),
            &cfg,
            cfg.check_test_fixtures,
        );
        if found.truncated.max_files_hit {
            return Err(miette::miette!(
                "{}: dump-facts cannot export a deterministic file set after max-files truncation; increase index.max-files or narrow the input",
                root.display()
            ));
        }
        found.truncated.oversized_files.sort();
        found.truncated.depth_pruned_dirs.sort();
        if found.truncated.any_hit() {
            check::report_truncation(&found.truncated, root);
            truncations.push(json!({
                "root": file_identity(root)?,
                "max_files_hit": found.truncated.max_files_hit,
                "oversized_files": found.truncated.oversized_files.iter().map(|(path, size)| {
                    Ok((utf8_path(path)?, size))
                }).collect::<Result<Vec<_>>>()?,
                "depth_pruned_dirs": found.truncated.depth_pruned_dirs.iter().map(|path| utf8_path(path)).collect::<Result<Vec<_>>>()?,
            }));
        }
        paths.extend(found.files);
    }
    check::sort_and_deduplicate_paths(&mut paths);
    for path in &paths {
        utf8_path(path)?;
    }
    let parsed = pipeline::parse_files(&paths, |_, _| pipeline::FailureAction::Abort)
        .map_err(|failure| miette::miette!("{}: {}", failure.path.display(), failure.error))?;
    let mut sources = BTreeMap::new();
    let mut canonical_files = BTreeSet::new();
    for file in &parsed {
        if !file.parse_errors.is_empty() {
            return Err(miette::miette!(
                "{}: dump-facts cannot export facts from source with syntax errors",
                file.path
            ));
        }
        // The shared parser accepts Latin-1. Facts must splice the original
        // bytes, so reject transcoded or concurrently modified input.
        let bytes = std::fs::read(&file.path).into_diagnostic()?;
        if bytes != file.source.as_bytes() {
            return Err(miette::miette!(
                "{}: dump-facts requires unchanged UTF-8 source; input is non-UTF-8 or changed while reading",
                file.path
            ));
        }
        let identity = file_identity(Path::new(&file.path))?;
        if !canonical_files.insert(identity.clone()) {
            return Err(miette::miette!(
                "dump-facts received the same physical file through multiple paths: {identity}"
            ));
        }
        sources.insert(
            file.path.clone(),
            json!({
                "path": identity,
                "source_hash": digest(&bytes),
            }),
        );
    }
    let user_stubs = check::load_user_stubs(&cfg.typeshed);
    let groups = pipeline::resolve_groups(
        &parsed,
        &cfg,
        &user_stubs,
        &[project_root.as_deref(), config_root.as_deref()],
    )?;
    let build =
        json!({"version": env!("CARGO_PKG_VERSION"), "executable_hash": executable_digest()?});
    let configuration = json!({
        "root": config_root.as_deref().map(file_identity).transpose()?,
        "effective": cfg,
        "environment_roots": cfg.environments.iter().map(|profile| &profile.root).collect::<Vec<_>>(),
    });
    let typeshed = json!({
        "bundled_source": ry_typeshed::SOURCE,
        "bundled_build_hash": build["executable_hash"],
        "custom_hash": json_digest(&custom_typeshed_value(&user_stubs)),
        "custom_packages": user_stubs.keys().collect::<Vec<_>>(),
    });
    let mut contexts = Vec::new();
    let mut exported = Vec::new();
    for group in groups {
        let input = group.check_input;
        let workspace = input
            .workspace
            .as_ref()
            .expect("resolved groups have workspace context");
        let mut context_sources: Vec<_> = input
            .files
            .iter()
            .map(|(path, _)| sources[path].clone())
            .collect();
        context_sources.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        let builtin_environments: BTreeMap<_, _> = input
            .files
            .iter()
            .map(|(path, _)| {
                (
                    sources[path]["path"].as_str().expect("canonical path"),
                    ry_checker::builtin_environment_bindings(path),
                )
            })
            .collect();
        let context = json!({
            "builtin_environments": builtin_environments,
            "root": file_identity(&group.resolution_root)?,
            "build": build,
            "config_hash": json_digest(&configuration),
            "typeshed": typeshed,
            "sources": context_sources,
            "workspace_hash": json_digest(&workspace_value(workspace)),
            "degraded_scopes": group.degraded_scopes.iter().map(|(path, reason)| {
                Ok((utf8_path(path)?, reason))
            }).collect::<Result<Vec<_>>>()?,
        });
        let context_id = json_digest(&context);
        contexts.push(json!({"id": context_id, "inputs": context}));
        let imported = workspace.imported_bindings.clone();
        let group_files = input.files.clone();
        let facts = check::check_project_with_facts_capture(input, references);
        let mut captures: HashMap<_, _> = facts.scopes.into_iter().collect();
        let mut reference_captures: HashMap<_, _> = facts.references.into_iter().collect();
        for (path, file) in group_files {
            if builtin_environments[sources[&path]["path"].as_str().expect("canonical path")]
                != ry_checker::builtin_environment_bindings(&path)
            {
                return Err(miette::miette!(
                    "{path}: built-in environment changed during analysis; retry dump-facts"
                ));
            }
            let records = captures.remove(&path).unwrap_or_default();
            let mut exported_file = json!({
                "path": sources[&path]["path"],
                "source_hash": sources[&path]["source_hash"],
                "context_id": context_id,
                "scopes": export_scopes(&file, records),
                "imports": imported.get(&path).map(|imports| imports.iter().collect::<BTreeMap<_,_>>()).unwrap_or_default(),
            });
            if references {
                let facts = reference_captures.remove(&path).unwrap_or_default();
                let (definitions, records) = export_references(&file, facts);
                exported_file["definitions"] = definitions;
                exported_file["references"] = records;
            }
            exported.push(exported_file);
        }
    }
    exported.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    contexts.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    truncations.sort_by_key(Value::to_string);
    let mut result = json!({
        "schema_version": 1,
        "producer": build,
        "snapshot_kind": "scope_exit",
        "coordinates": {"encoding": "utf-8", "bytes": "zero_based_half_open", "line_column": "one_based_unicode_scalar", "tab_width": 1},
        "configuration": configuration,
        "contexts": contexts,
        "discovery": {"complete": truncations.is_empty(), "truncations": truncations},
        "files": exported,
    });
    if references {
        result["schema_version"] = json!(2);
        result
            .as_object_mut()
            .expect("facts object")
            .remove("snapshot_kind");
        result["scope_snapshot_kind"] = json!("scope_exit");
        result["capabilities"] = json!({
            "scope_snapshots": true,
            "reference_facts": "same_file_ordered_prefix",
            "reference_coverage": "partial",
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&result).into_diagnostic()?
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ry_checker::Scope;
    use ry_core::{RParser, RType};

    #[test]
    fn synthetic_spans_and_scope_uncertainty_remain_explicit() {
        let file = RParser::new()
            .unwrap()
            .parse("sample.R", "x <- 1L\n")
            .unwrap();
        let mut scope = Scope::default();
        scope.data_mask_unknown = true;
        scope.search_path_unknown = true;
        scope.bindings.insert("synthetic".into(), RType::unknown());
        scope.parameter_bindings.insert("synthetic".into());
        let record = ScopeRecord {
            kind: ScopeRecordKind::Function,
            name: None,
            span: Span::default(),
            params: vec![("synthetic".into(), Span::default())],
            scope,
        };
        let output = export_scopes(&file, vec![record]);
        assert!(output[0]["span"].is_null());
        assert_eq!(output[0]["data_mask_unknown"], true);
        assert_eq!(output[0]["search_path_unknown"], true);
        let declaration = &output[0]["bindings"][0]["declaration"];
        assert_eq!(declaration["kind"], "unavailable");
        assert!(declaration["span"].is_null());
    }

    #[test]
    fn invalid_byte_boundaries_never_become_source_spans() {
        assert!(source_span("é", Span::new(1, 2, 0, 1)).is_null());
        assert!(source_span("x", Span::new(0, 2, 0, 0)).is_null());
        assert!(source_span("x", Span::new(1, 0, 0, 1)).is_null());
        assert_eq!(
            source_span("é", Span::new(0, 2, 0, 0))["end"],
            json!([1, 2])
        );
    }
}
