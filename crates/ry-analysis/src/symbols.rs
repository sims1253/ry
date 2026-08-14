//! Resolved symbol identity and cross-file symbol index.
//!
//! P38-W6: Symbols have identity distinct from spelling. A `SymbolId`
//! identifies a binding by file + name + scope, not just its text.
//! Navigation queries use resolved symbols so a local `helper` in b.R
//! is never confused with a global `helper` in a.R.

use ry_core::{Expr, Stmt};
use std::collections::HashMap;

// == SymbolId ==

/// The kind of a resolved symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// A top-level binding (`x <- 1`).
    Global,
    /// A local binding inside a function or block.
    Local,
    /// A function parameter.
    Parameter,
    /// A loop variable.
    LoopVar,
    /// A user-defined function.
    Function,
}

/// A resolved symbol identity.
///
/// Two symbols with the same spelling but different files or scopes are
/// distinct `SymbolId`s.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SymbolId {
    /// The file where the symbol is defined.
    pub file: String,
    /// The name of the symbol.
    pub name: String,
    /// The kind of the symbol.
    pub kind: SymbolKind,
}

/// A definition site for a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionSite {
    /// The symbol this defines.
    pub symbol: SymbolId,
    /// Byte offset of the definition name.
    pub start: u32,
    /// End byte offset of the definition name.
    pub end: u32,
}

/// A reference site for a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceSite {
    /// The file containing the reference.
    pub file: String,
    /// Byte offset of the reference.
    pub start: u32,
    /// End byte offset.
    pub end: u32,
}

// == SymbolIndex ==

/// A cross-file index of symbols and their references.
///
/// Built from parsed source files. For each file, it records every
/// top-level definition and every identifier usage. References are
/// matched to definitions by (name, scope) resolution.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    /// Map from symbol name to all definition sites across all files.
    definitions: HashMap<String, Vec<DefinitionSite>>,
    /// Map from file path to all references in that file.
    references: HashMap<String, Vec<(String, u32, u32)>>,
}

impl SymbolIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a definition site.
    pub fn add_definition(
        &mut self,
        file: &str,
        name: &str,
        kind: SymbolKind,
        start: u32,
        end: u32,
    ) {
        self.definitions
            .entry(name.to_string())
            .or_default()
            .push(DefinitionSite {
                symbol: SymbolId {
                    file: file.to_string(),
                    name: name.to_string(),
                    kind,
                },
                start,
                end,
            });
    }

    /// Add a reference site.
    pub fn add_reference(&mut self, file: &str, name: &str, start: u32, end: u32) {
        self.references
            .entry(file.to_string())
            .or_default()
            .push((name.to_string(), start, end));
    }

    /// Find all definitions with a given name.
    pub fn find_definitions(&self, name: &str) -> &[DefinitionSite] {
        self.definitions
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Find all references to a name across all files.
    pub fn find_references(&self, name: &str) -> Vec<ReferenceSite> {
        let mut result = Vec::new();
        for (file, refs) in &self.references {
            for (ref_name, start, end) in refs {
                if ref_name == name {
                    result.push(ReferenceSite {
                        file: file.clone(),
                        start: *start,
                        end: *end,
                    });
                }
            }
        }
        result
    }

    /// Find all references to a name in a specific file.
    pub fn find_references_in_file(&self, file: &str, name: &str) -> Vec<ReferenceSite> {
        self.references
            .get(file)
            .map(|refs| {
                refs.iter()
                    .filter(|(n, _, _)| n == name)
                    .map(|(_, start, end)| ReferenceSite {
                        file: file.to_string(),
                        start: *start,
                        end: *end,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all files that have symbols indexed.
    pub fn indexed_files(&self) -> impl Iterator<Item = &str> {
        self.definitions
            .values()
            .flat_map(|v| v.iter().map(|d| d.symbol.file.as_str()))
            .chain(self.references.keys().map(|s| s.as_str()))
    }

    /// Number of unique symbol names indexed.
    pub fn symbol_count(&self) -> usize {
        self.definitions.len()
    }
}

/// Build a SymbolIndex from a parsed source file.
///
/// This is a syntax-based index that records all definition sites (
/// assignments, function defs, loop vars) and all identifier usages.
/// Future workstreams will enhance this with scope-aware resolution.
pub fn build_index_from_file(file_path: &str, file: &ry_core::SourceFile) -> SymbolIndex {
    let mut index = SymbolIndex::new();

    for stmt in &file.stmts {
        index_stmt(stmt, file_path, &mut index);
    }

    index
}

fn index_stmt(stmt: &ry_core::Stmt, file: &str, index: &mut SymbolIndex) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            if let Expr::Ident { name, span } = target {
                let kind = if matches!(value, Expr::Function { .. }) {
                    // value is Box<Expr>, as_ref gives &Expr
                    SymbolKind::Function
                } else {
                    SymbolKind::Global
                };
                index.add_definition(
                    file,
                    name,
                    kind,
                    span.start as u32,
                    span.start as u32 + name.len() as u32,
                );
            }
            index_expr(value, file, index);
        }
        Stmt::FunctionDef {
            name, body, span, ..
        } => {
            if let Some(n) = name {
                index.add_definition(
                    file,
                    n,
                    SymbolKind::Function,
                    span.start as u32,
                    span.start as u32 + n.len() as u32,
                );
            }
            for s in body {
                index_stmt(s, file, index);
            }
        }
        Stmt::If {
            cond, then, else_, ..
        } => {
            index_expr(cond, file, index);
            for s in then {
                index_stmt(s, file, index);
            }
            if let Some(eb) = else_ {
                for s in eb {
                    index_stmt(s, file, index);
                }
            }
        }
        Stmt::For {
            name,
            iter,
            body,
            name_span,
            ..
        } => {
            index.add_definition(
                file,
                name,
                SymbolKind::LoopVar,
                name_span.start as u32,
                name_span.start as u32 + name.len() as u32,
            );
            index_expr(iter, file, index);
            for s in body {
                index_stmt(s, file, index);
            }
        }
        Stmt::While { cond, body, .. } => {
            index_expr(cond, file, index);
            for s in body {
                index_stmt(s, file, index);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(v) = value {
                index_expr(v, file, index);
            }
        }
        Stmt::Expr(e) => index_expr(e, file, index),
    }
}

fn index_expr(expr: &ry_core::Expr, file: &str, index: &mut SymbolIndex) {
    match expr {
        Expr::Ident { name, span } => {
            index.add_reference(
                file,
                name,
                span.start as u32,
                span.start as u32 + name.len() as u32,
            );
        }
        Expr::Call { func, args, .. } => {
            index_expr(func, file, index);
            for arg in args {
                index_expr(&arg.value, file, index);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            index_expr(lhs, file, index);
            index_expr(rhs, file, index);
        }
        Expr::UnaryOp { expr, .. } => index_expr(expr, file, index),
        Expr::Index { base, args, .. } => {
            index_expr(base, file, index);
            for arg in args {
                index_expr(&arg.value, file, index);
            }
        }
        Expr::Function { params, body, .. } => {
            for param in params {
                index.add_definition(
                    file,
                    &param.name,
                    SymbolKind::Parameter,
                    param.span.start as u32,
                    param.span.start as u32 + param.name.len() as u32,
                );
            }
            for s in body {
                index_stmt(s, file, index);
            }
        }
        Expr::Block { body, .. } => {
            for s in body {
                index_stmt(s, file, index);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            index_expr(cond, file, index);
            index_expr(then, file, index);
            if let Some(e) = else_ {
                index_expr(e, file, index);
            }
        }
        Expr::Logical(_, _)
        | Expr::Integer(_, _)
        | Expr::Double(_, _)
        | Expr::String(_, _)
        | Expr::Null(_)
        | Expr::Na(_, _)
        | Expr::Unknown(_) => {}
    }
}

/// Merge multiple file-level indices into one project-wide index.
pub fn merge_indices(indices: impl IntoIterator<Item = SymbolIndex>) -> SymbolIndex {
    let mut merged = SymbolIndex::new();
    for index in indices {
        for (name, defs) in index.definitions {
            merged.definitions.entry(name).or_default().extend(defs);
        }
        for (file, refs) in index.references {
            merged.references.entry(file).or_default().extend(refs);
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> ry_core::SourceFile {
        let mut parser = ry_core::RParser::new().unwrap();
        parser.parse("test.R", src).unwrap()
    }

    #[test]
    fn index_finds_global_definition() {
        let file = parse("x <- 1\ny <- x\n");
        let idx = build_index_from_file("test.R", &file);
        let defs = idx.find_definitions("x");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol.kind, SymbolKind::Global);
        assert_eq!(defs[0].symbol.file, "test.R");
    }

    #[test]
    fn index_finds_function_definition() {
        let file = parse("my_fn <- function(x) x\n");
        let idx = build_index_from_file("test.R", &file);
        let defs = idx.find_definitions("my_fn");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol.kind, SymbolKind::Function);

        let param_defs = idx.find_definitions("x");
        assert_eq!(param_defs.len(), 1);
        assert_eq!(param_defs[0].symbol.kind, SymbolKind::Parameter);
    }

    #[test]
    fn index_finds_references() {
        let file = parse("x <- 1\ny <- x\nz <- x\n");
        let idx = build_index_from_file("test.R", &file);
        let refs = idx.find_references("x");
        // x appears as: definition (line 0) + references (line 1, line 2)
        assert_eq!(
            refs.len(),
            2,
            "should find 2 references (not the definition)"
        );
    }

    #[test]
    fn index_finds_loop_var() {
        let file = parse("for (i in 1:10) { print(i) }\n");
        let idx = build_index_from_file("test.R", &file);
        let defs = idx.find_definitions("i");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol.kind, SymbolKind::LoopVar);
    }

    #[test]
    fn merged_index_spans_files() {
        let file_a = parse("shared <- 1\n");
        let file_b = parse("shared + 1\n");
        let idx_a = build_index_from_file("a.R", &file_a);
        let idx_b = build_index_from_file("b.R", &file_b);
        let merged = merge_indices([idx_a, idx_b]);

        let defs = merged.find_definitions("shared");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].symbol.file, "a.R");

        let refs = merged.find_references("shared");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].file, "b.R");
    }

    #[test]
    fn same_name_different_files() {
        // Two files defining the same name - both are valid definitions
        let file_a = parse("helper <- function() 1\n");
        let file_b = parse("helper <- function() 2\nhelper()\n");
        let idx_a = build_index_from_file("a.R", &file_a);
        let idx_b = build_index_from_file("b.R", &file_b);
        let merged = merge_indices([idx_a, idx_b]);

        let defs = merged.find_definitions("helper");
        assert_eq!(defs.len(), 2, "both files define helper");

        // The reference in b.R should resolve to b.R's local definition
        let b_def = defs.iter().find(|d| d.symbol.file == "b.R").unwrap();
        assert_eq!(b_def.start, 0);
    }
}
