//! `ry dump-types`: one analysis pass over the requested files, then a
//! JSON dump of every recorded lexical scope (see ry_checker::ScopeRecord).
//! The dump reuses `ry check`'s pipeline end to end — config discovery,
//! file discovery, per-package grouping, workspace resolution — so the
//! emitted types are exactly what a `ry check` run infers.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::process::ExitCode;

use miette::{IntoDiagnostic, Result};
use ry_core::ast::{BinOpKind, Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};

use crate::check::{self, load_user_stubs, report_truncation, sort_and_deduplicate_paths};
use crate::pipeline;

#[derive(serde::Serialize)]
struct TypesDump {
    files: Vec<FileDump>,
}

#[derive(serde::Serialize)]
struct FileDump {
    path: String,
    scopes: Vec<ScopeDump>,
}

#[derive(serde::Serialize)]
struct ScopeDump {
    kind: &'static str,
    name: Option<String>,
    start: (usize, usize),
    end: (usize, usize),
    bindings: Vec<BindingDump>,
}

#[derive(serde::Serialize)]
struct BindingDump {
    name: String,
    kind: &'static str,
    #[serde(rename = "type")]
    type_: String,
    start: Option<(usize, usize)>,
}

/// clap value parser for `--position LINE:COL`. Rows and columns are
/// 1-based, matching the dump output.
pub(crate) fn parse_dump_position(value: &str) -> Result<(usize, usize), String> {
    let (line, col) = value
        .split_once(':')
        .ok_or_else(|| format!("expected LINE:COL, got `{value}`"))?;
    let line = line
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid line in `{value}`"))?;
    let col = col
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid column in `{value}`"))?;
    if line == 0 || col == 0 {
        return Err(format!("positions are 1-based, got `{value}`"));
    }
    Ok((line, col))
}

/// The type string for one binding. Same `Display` rendering the LSP
/// inlay hints show, except the fully-uninformed type is reported as
/// "unknown" so consumers never mistake `opaque<len=?>:?` for a real
/// inference result.
fn dump_type_string(t: &ry_core::RType) -> String {
    if *t == ry_core::RType::unknown() {
        "unknown".to_string()
    } else {
        t.to_string()
    }
}

/// 1-based (row, character-column) of a byte offset. Parser spans use
/// byte columns; converting to character columns keeps the dump useful
/// for files with multi-byte identifiers.
fn offset_to_line_char_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let row = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = source[line_start..offset].chars().count() + 1;
    (row, col)
}

/// Inverse of [`offset_to_line_char_col`]: byte offset of the 1-based
/// (row, character-column) position. `None` when the row is past the
/// last line; a column past the line end clamps to the line end.
fn line_char_col_to_offset(source: &str, row: usize, col: usize) -> Option<usize> {
    let mut offset = 0usize;
    for (index, line) in source.split('\n').enumerate() {
        if index + 1 == row {
            let mut bytes = 0usize;
            for ch in line.chars().take(col.saturating_sub(1)) {
                bytes += ch.len_utf8();
            }
            return Some((offset + bytes).min(offset + line.len()));
        }
        offset += line.len() + 1;
    }
    None
}

/// Record the first plain assignment to each name in a scope's body.
///
/// R has no separate block scoping, so assignments inside `if`/`for`/
/// `while` bodies and braced value blocks bind in the enclosing function
/// scope. Runs on the shared walker; skips function bodies and
/// assignment targets (indexed targets mutate rather than bind, and the
/// checker does not bind assignments hidden in a target's index
/// arguments, so the dump reports exactly what a `ry check` run
/// infers). The *name* bound to a function literal
/// (`inner <- function(...)`) is itself a local of this scope.
fn collect_local_bindings(stmts: &[ry_core::ast::Stmt], out: &mut HashMap<String, ry_core::Span>) {
    let _ = walk_stmts(
        stmts,
        Walk {
            assign_targets: false,
            fn_bodies: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::Assign {
                    target: Expr::Ident { name, span } | Expr::String(name, span),
                    ..
                }) => {
                    out.entry(name.clone()).or_insert(*span);
                }
                AstNode::Stmt(Stmt::For {
                    name, name_span, ..
                }) => {
                    out.entry(name.clone()).or_insert(*name_span);
                }
                // Assignment operators in expression position bind the
                // LHS in the current scope: `<-`/`<<-` (R's `<-` returns
                // the value invisibly) and `%<>%` (which rebinds its LHS
                // ident). The LHS subtree is still walked: a nested
                // `a <- b <- 1L` chain binds both names.
                AstNode::Expr(Expr::BinOp {
                    op: BinOpKind::Assign | BinOpKind::SuperAssign | BinOpKind::PipeAssign,
                    lhs,
                    ..
                }) => {
                    if let Expr::Ident { name, span } | Expr::String(name, span) = lhs.as_ref() {
                        out.entry(name.clone()).or_insert(*span);
                    }
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

/// Map every function body in the file to its start byte, so each
/// recorded scope can look up its own local bindings. Walks into
/// function bodies (that is what is being indexed) but skips assignment
/// targets; each discovered body is indexed and pruned in one step.
fn index_scope_bodies(
    stmts: &[ry_core::ast::Stmt],
    index: &mut HashMap<usize, HashMap<String, ry_core::Span>>,
) {
    let _ = walk_stmts(
        stmts,
        Walk {
            assign_targets: false,
            ..Walk::ALL
        },
        |node: AstNode<'_>, _: usize| -> ControlFlow<(), Descend> {
            match node {
                AstNode::Stmt(Stmt::FunctionDef { body, span, .. }) => {
                    index_function_body(*span, body, index);
                    return ControlFlow::Continue(Descend::Skip);
                }
                AstNode::Stmt(Stmt::Assign {
                    value: Expr::Function { body, span, .. },
                    ..
                }) => {
                    index_function_body(*span, body, index);
                    return ControlFlow::Continue(Descend::Skip);
                }
                _ => {}
            }
            ControlFlow::Continue(Descend::Into)
        },
    );
}

fn index_function_body(
    span: ry_core::Span,
    body: &[ry_core::ast::Stmt],
    index: &mut HashMap<usize, HashMap<String, ry_core::Span>>,
) {
    let mut locals = HashMap::new();
    collect_local_bindings(body, &mut locals);
    index.insert(span.start, locals);
    index_scope_bodies(body, index);
}

/// Per-scope classification inputs derived once per file.
struct ScopeInfo<'a> {
    record: &'a ry_checker::ScopeRecord,
    params: HashMap<&'a str, ry_core::Span>,
    locals: HashMap<&'a str, ry_core::Span>,
}

/// Turn one file's scope records into the JSON dump shape.
///
/// Binding kinds (documented in the README section for `dump-types`):
/// - `param`: still marked as a formal in the recorded scope (an
///   overwritten formal degrades to `local`, matching R's rebinding).
/// - `local`: first assigned inside this scope's own body.
/// - `closed-over`: function scopes only — present because the body's
///   scope was cloned from the enclosing one at definition time.
/// - `imported`: top-level bindings the file never assigns (ambient names
///   supplied by the host environment, e.g. Shiny server fragments).
fn assemble_file_dump(
    path: &str,
    file: &ry_core::SourceFile,
    records: Vec<ry_checker::ScopeRecord>,
    positions: &[(usize, usize)],
) -> FileDump {
    // Sort by start position and drop duplicates (an injected-expression
    // re-walk can complete the same literal twice). The dedup key
    // includes the kind: a leading function literal's span starts at the
    // same byte 0 as the whole-file top scope, and keying on the offset
    // alone would drop that top scope and every top-level binding with
    // it.
    let mut records = records;
    records.sort_by_key(|record| record.span.start);
    records.dedup_by(|a, b| a.kind == b.kind && a.span.start == b.span.start);

    let mut function_locals: HashMap<usize, HashMap<String, ry_core::Span>> = HashMap::new();
    index_scope_bodies(&file.stmts, &mut function_locals);
    let mut top_locals = HashMap::new();
    collect_local_bindings(&file.stmts, &mut top_locals);

    let infos: Vec<ScopeInfo> = records
        .iter()
        .map(|record| ScopeInfo {
            record,
            params: record
                .params
                .iter()
                .map(|(name, span)| (name.as_str(), *span))
                .collect(),
            locals: match record.kind {
                ry_checker::ScopeRecordKind::Function => function_locals
                    .get(&record.span.start)
                    .map(|locals| {
                        locals
                            .iter()
                            .map(|(name, span)| (name.as_str(), *span))
                            .collect()
                    })
                    .unwrap_or_default(),
                ry_checker::ScopeRecordKind::Top => top_locals
                    .iter()
                    .map(|(name, span)| (name.as_str(), *span))
                    .collect(),
            },
        })
        .collect();

    // Enclosing-scope chains are shared by every binding of a scope, so
    // compute them once per file instead of once per closed-over lookup.
    let chains = enclosing_scope_chains(&infos);

    // Which scopes does --position select? Without positions, all. With
    // them, the innermost containing scope for each position (the union,
    // deduplicated). Byte offsets make containment exact regardless of
    // encoding. One pass records both the selected scopes and, per
    // scope, the offsets that selected it — the latter drives
    // binding-visibility filtering below.
    let mut selected: Vec<usize> = Vec::new();
    let mut selecting_offsets: HashMap<usize, Vec<usize>> = HashMap::new();
    if positions.is_empty() {
        selected.extend(0..infos.len());
    } else {
        for &(row, col) in positions {
            let Some(offset) = line_char_col_to_offset(&file.source, row, col) else {
                continue;
            };
            let mut best: Option<(usize, usize)> = None;
            for (index, info) in infos.iter().enumerate() {
                let span = info.record.span;
                if span.start <= offset && offset < span.end {
                    let extent = span.end - span.start;
                    if best.is_none_or(|(extent_so_far, _)| extent < extent_so_far) {
                        best = Some((extent, index));
                    }
                }
            }
            if let Some((_, index)) = best {
                selecting_offsets.entry(index).or_default().push(offset);
            }
        }
        selected.extend(selecting_offsets.keys().copied());
        selected.sort_unstable();
        selected.dedup();
    }

    let scopes = selected
        .into_iter()
        .map(|index| {
            let info = &infos[index];
            let record = info.record;
            let source = &file.source;

            let mut bindings: Vec<BindingDump> = record
                .scope
                .bindings
                .iter()
                .map(|(name, ty)| {
                    // A formal counts as `param` only in the scope that
                    // declares it: scope snapshots are cloned from the
                    // enclosing function, which also clones the
                    // parameter-marker set, so a captured outer formal
                    // must not be reclassified. A declared formal whose
                    // marker is gone was reassigned in the body (R
                    // rebinds rather than narrows), so it degrades to
                    // `local` at its reassignment site.
                    let is_formal_here = info.params.contains_key(name.as_str());
                    let is_param = is_formal_here && record.scope.parameter_bindings.contains(name);
                    let local_span = info.locals.get(name.as_str()).copied();
                    let (kind, definition_span) = if is_param {
                        (
                            "param",
                            info.params.get(name.as_str()).copied().or(local_span),
                        )
                    } else if let Some(span) = local_span {
                        ("local", Some(span))
                    } else if record.kind == ry_checker::ScopeRecordKind::Function {
                        ("closed-over", None)
                    } else {
                        ("imported", None)
                    };
                    // A closed-over binding has no site in this scope;
                    // point at the definition in the nearest enclosing
                    // scope that binds the name, when one was recorded.
                    let definition_span = definition_span
                        .or_else(|| enclosing_binding_span(&infos, &chains[index], name.as_str()));
                    (name, ty, kind, definition_span, is_formal_here)
                })
                .filter(|(_, _, _, definition_span, is_formal_here)| {
                    // Visibility at the selecting positions: formals bind
                    // at call entry (always visible); a local assigned
                    // after every selecting position is not yet in scope
                    // there; a binding with no resolvable site is kept
                    // (it predates the body).
                    let Some(offsets) = selecting_offsets.get(&index) else {
                        return true;
                    };
                    if *is_formal_here {
                        return true;
                    }
                    let Some(span) = definition_span else {
                        return true;
                    };
                    offsets.iter().any(|offset| span.start <= *offset)
                })
                .map(|(name, ty, kind, definition_span, _)| BindingDump {
                    name: name.clone(),
                    kind,
                    type_: dump_type_string(ty),
                    start: definition_span.map(|span| offset_to_line_char_col(source, span.start)),
                })
                .collect();
            bindings.sort_by(|a, b| a.name.cmp(&b.name));

            ScopeDump {
                kind: match record.kind {
                    ry_checker::ScopeRecordKind::Function => "function",
                    ry_checker::ScopeRecordKind::Top => "top",
                },
                name: record.name.clone(),
                start: offset_to_line_char_col(source, record.span.start),
                end: offset_to_line_char_col(source, record.span.end),
                bindings,
            }
        })
        .collect();

    FileDump {
        path: path.to_string(),
        scopes,
    }
}

/// Sorted (nearest-first) enclosing-scope index chain for every scope:
/// each entry lists the other scopes whose span contains it, ordered by
/// smallest extent first. Computed once per file so every binding lookup
/// in a scope reuses the same chain.
fn enclosing_scope_chains(infos: &[ScopeInfo]) -> Vec<Vec<usize>> {
    (0..infos.len())
        .map(|index| {
            let inner = infos[index].record.span;
            let mut chain: Vec<usize> = (0..infos.len())
                .filter(|other| {
                    *other != index
                        && infos[*other].record.span.start <= inner.start
                        && inner.end <= infos[*other].record.span.end
                })
                .collect();
            // Nearest first: smallest enclosing extent.
            chain.sort_by_key(|other| {
                infos[*other].record.span.end - infos[*other].record.span.start
            });
            chain
        })
        .collect()
}

/// Definition site of `name` in the nearest recorded scope enclosing the
/// scope that owns `chain`, walking outward. Used for closed-over
/// bindings, whose only site in this file lives in an outer scope.
fn enclosing_binding_span(
    infos: &[ScopeInfo],
    chain: &[usize],
    name: &str,
) -> Option<ry_core::Span> {
    for index in chain {
        let info = &infos[*index];
        if let Some(span) = info.params.get(name).copied() {
            return Some(span);
        }
        if let Some(span) = info.locals.get(name).copied() {
            return Some(span);
        }
    }
    None
}

/// dump-types' parse-failure policy: an unparseable file is warned
/// about and omitted from the dump; an unreadable file aborts the run.
/// The abort is reported by the caller after the join, so exactly one
/// read error is printed per run.
fn dump_parse_failure(
    path: &std::path::Path,
    error: &pipeline::ParseError,
) -> pipeline::FailureAction {
    match error {
        pipeline::ParseError::Read(_) => pipeline::FailureAction::Abort,
        pipeline::ParseError::Parse(message) => {
            eprintln!(
                "ry: {}: parse error: {message}; file omitted from dump",
                path.display()
            );
            pipeline::FailureAction::Skip
        }
    }
}

pub(crate) fn run_dump_types(
    files: Vec<PathBuf>,
    project_root: Option<PathBuf>,
    format: &str,
    positions: Vec<(usize, usize)>,
) -> Result<ExitCode> {
    if format != "json" {
        return Err(miette::miette!(
            "unknown --format `{}`; only `json` is supported",
            format
        ));
    }

    // Config discovery mirrors `ry check` through the shared helper:
    // missing config is fine, malformed config aborts. The config's
    // directory is kept as `config_root` — it anchors the `exclude`
    // patterns below and is the resolution-root fallback for non-package
    // files, exactly as in `run_check`. The anchor differs from
    // `ry check` on purpose: dump-types requires files, and discovery
    // starts at the first input itself, not at its parent.
    let search_start = files.first().cloned().unwrap_or_else(|| PathBuf::from("."));
    let (config_root, cfg) = match pipeline::discover_config(&search_start) {
        Ok(found) => found,
        Err(code) => return Ok(code),
    };

    let mut all_paths = Vec::new();
    for root in &files {
        if !root.exists() {
            eprintln!("ry: {}: no such file or directory", root.display());
            return Ok(ExitCode::FAILURE);
        }
        let result = ry_workspace::discover_r_files(
            root,
            config_root.as_deref(),
            &cfg,
            cfg.check_test_fixtures,
        );
        all_paths.extend(result.files);
        report_truncation(&result.truncated, root);
    }
    sort_and_deduplicate_paths(&mut all_paths);

    // Parsing goes through the same path as `ry check`; see
    // `pipeline::parse_files`.
    let parsed = match pipeline::parse_files(&all_paths, dump_parse_failure) {
        Ok(parsed) => parsed,
        Err(failure) => {
            eprintln!("ry: {}: {}", failure.path.display(), failure.error);
            return Ok(ExitCode::FAILURE);
        }
    };

    let user_stubs = load_user_stubs(&cfg.typeshed);

    // Same per-package grouping as `ry check`: each DESCRIPTION root is
    // its own library namespace. Non-package scripts share one group
    // rooted at --project-root, else the config root (the directory
    // owning the discovered ry.toml), else the working directory —
    // `run_check_once`'s fallback chain with --project-root overriding.
    let groups = pipeline::resolve_groups(
        &parsed,
        &cfg,
        &user_stubs,
        &[project_root.as_deref(), config_root.as_deref()],
    )?;

    let mut records_by_path: HashMap<String, Vec<ry_checker::ScopeRecord>> = HashMap::new();
    for group in groups {
        // `ry check` prints one summary line per degraded scope; keep the
        // note on stderr here too so a dump over the same project reports
        // the same precision loss without polluting the JSON on stdout.
        for (path, reason) in &group.degraded_scopes {
            eprintln!(
                "ry: {}: degraded scope ({reason}); serialized data file(s) over the byte cap fell back to file stems",
                path.display()
            );
        }
        for (path, records) in check::check_project_with_scope_capture(group.check_input) {
            records_by_path.insert(path, records);
        }
    }

    let dump = TypesDump {
        files: parsed
            .iter()
            .map(|parsed_file| {
                let records = records_by_path
                    .remove(&parsed_file.path)
                    .unwrap_or_default();
                assemble_file_dump(&parsed_file.path, &parsed_file.file, records, &positions)
            })
            .collect(),
    };
    println!("{}", serde_json::to_string_pretty(&dump).into_diagnostic()?);
    // Diagnostics (if any) never affect the dump's exit code.
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dump_position_parser_accepts_pairs_and_rejects_garbage() {
        assert_eq!(parse_dump_position("3:14").unwrap(), (3, 14));
        assert_eq!(parse_dump_position(" 12 : 1 ").unwrap(), (12, 1));
        assert!(parse_dump_position("3").is_err());
        assert!(parse_dump_position("a:b").is_err());
        assert!(parse_dump_position("0:1").is_err(), "rows are 1-based");
        assert!(parse_dump_position("1:0").is_err(), "cols are 1-based");
    }

    #[test]
    fn dump_position_offsets_round_trip_and_clamp() {
        // Multi-byte characters: columns must count characters, not bytes.
        let src = "a <- 1L\n#\u{e9} <- 2L\nlast <- 3L\n";
        assert_eq!(offset_to_line_char_col(src, 0), (1, 1));
        // Start of line 3.
        assert_eq!(
            offset_to_line_char_col(src, src.find("last").unwrap()),
            (3, 1)
        );
        // The identifier on line 2 starts after `#`, a multi-byte char.
        let ident = src.find("<- 2L").unwrap();
        let (row, col) = offset_to_line_char_col(src, ident);
        assert_eq!((row, col), (2, 4));
        assert_eq!(
            line_char_col_to_offset(src, row, col),
            Some(ident),
            "round trip"
        );
        // Row past the end matches nothing; column past the line end
        // clamps to the line end.
        assert_eq!(line_char_col_to_offset(src, 99, 1), None);
        assert_eq!(
            line_char_col_to_offset(src, 1, 500),
            Some(src.find('\n').unwrap())
        );
    }

    #[test]
    fn dump_type_string_renders_unknown_and_display_forms() {
        assert_eq!(dump_type_string(&ry_core::RType::unknown()), "unknown");
        let integer =
            ry_core::RType::new(ry_core::types::Mode::Integer, ry_core::types::Length::One);
        assert_eq!(dump_type_string(&integer), "integer<len=1>");
    }

    #[test]
    fn dump_local_binding_scan_skips_nested_function_bodies() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "a.R",
                "outer <- function(x) {\n  keep <- 1L\n  inner <- function(y) { skip <- 2L }\n  if (x) keep2 <- 3L\n  for (i in 1:3) keep3 <- i\n  keep\n}\n",
            )
            .unwrap();
        // The function body's own locals, extracted from the statement
        // tree: nested `skip` belongs to the inner scope, not this one.
        let ry_core::ast::Stmt::Assign { value, .. } = &file.stmts[0] else {
            panic!("expected assignment");
        };
        let ry_core::ast::Expr::Function { body, .. } = value else {
            panic!("expected function literal");
        };
        let mut locals = std::collections::HashMap::new();
        collect_local_bindings(body, &mut locals);
        let mut names: Vec<_> = locals.keys().map(String::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["i", "inner", "keep", "keep2", "keep3"]);
    }
}
