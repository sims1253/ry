//! Static R package metadata extracted from R source and NAMESPACE files.
//!
//! This module establishes whether package-provided names are in scope. It
//! intentionally does not assign precise types: imported or attached exports
//! become opaque bindings unless an embedded typeshed can refine them.

use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use std::collections::HashSet;

/// Bindings and whole-package imports declared by an R package NAMESPACE.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NamespaceMetadata {
    /// Names introduced by `importFrom(package, name, ...)`.
    pub imported_bindings: HashSet<String>,
    /// Packages introduced by `import(package, ...)`.
    pub imported_packages: HashSet<String>,
    /// Names made public by `export(name, ...)`.
    pub exports: HashSet<String>,
}

/// Extract the directives relevant to static binding resolution.
pub fn namespace_metadata(file: &SourceFile) -> NamespaceMetadata {
    let mut metadata = NamespaceMetadata::default();
    for stmt in &file.stmts {
        let Stmt::Expr(Expr::Call { func, args, .. }) = stmt else {
            continue;
        };
        let Expr::Ident { name, .. } = func.as_ref() else {
            continue;
        };
        match name.as_str() {
            "importFrom" => {
                metadata.imported_bindings.extend(
                    args.iter()
                        .skip(1)
                        .filter_map(|arg| static_name(&arg.value)),
                );
            }
            "import" => {
                metadata
                    .imported_packages
                    .extend(args.iter().filter_map(|arg| static_name(&arg.value)));
            }
            "export" => {
                metadata
                    .exports
                    .extend(args.iter().filter_map(|arg| static_name(&arg.value)));
            }
            _ => {}
        }
    }
    metadata
}

/// Find packages attached by `library()` or `require()` calls.
///
/// `requireNamespace()` is deliberately excluded: it makes `pkg::name`
/// available but does not place `name` on R's search path.
pub fn attached_packages(file: &SourceFile) -> HashSet<String> {
    let mut packages = HashSet::new();
    for stmt in &file.stmts {
        visit_stmt_for_attachments(stmt, &mut packages);
    }
    packages
}

fn static_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
        _ => None,
    }
}

fn visit_stmt_for_attachments(stmt: &Stmt, packages: &mut HashSet<String>) {
    match stmt {
        Stmt::Assign { target, value, .. } => {
            visit_expr_for_attachments(target, packages);
            visit_expr_for_attachments(value, packages);
        }
        Stmt::Expr(expr) => visit_expr_for_attachments(expr, packages),
        Stmt::If {
            cond, then, else_, ..
        } => {
            visit_expr_for_attachments(cond, packages);
            for stmt in then {
                visit_stmt_for_attachments(stmt, packages);
            }
            if let Some(else_) = else_ {
                for stmt in else_ {
                    visit_stmt_for_attachments(stmt, packages);
                }
            }
        }
        Stmt::For { iter, body, .. }
        | Stmt::While {
            cond: iter, body, ..
        } => {
            visit_expr_for_attachments(iter, packages);
            for stmt in body {
                visit_stmt_for_attachments(stmt, packages);
            }
        }
        Stmt::FunctionDef { body, .. } => {
            for stmt in body {
                visit_stmt_for_attachments(stmt, packages);
            }
        }
        Stmt::Return { value, .. } => {
            if let Some(value) = value {
                visit_expr_for_attachments(value, packages);
            }
        }
    }
}

fn visit_expr_for_attachments(expr: &Expr, packages: &mut HashSet<String>) {
    match expr {
        Expr::Call { func, args, .. } => {
            if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "library" || name == "require")
            {
                if let Some(package) = args.first().and_then(|arg| static_name(&arg.value)) {
                    packages.insert(package);
                }
            }
            visit_expr_for_attachments(func, packages);
            for arg in args {
                visit_expr_for_attachments(&arg.value, packages);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            visit_expr_for_attachments(lhs, packages);
            visit_expr_for_attachments(rhs, packages);
        }
        Expr::UnaryOp { expr, .. } => visit_expr_for_attachments(expr, packages),
        Expr::Index { base, args, .. } => {
            visit_expr_for_attachments(base, packages);
            for arg in args {
                visit_expr_for_attachments(&arg.value, packages);
            }
        }
        Expr::Function { body, .. } | Expr::Block { body, .. } => {
            for stmt in body {
                visit_stmt_for_attachments(stmt, packages);
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            visit_expr_for_attachments(cond, packages);
            visit_expr_for_attachments(then, packages);
            if let Some(else_) = else_ {
                visit_expr_for_attachments(else_, packages);
            }
        }
        Expr::Logical(..)
        | Expr::Integer(..)
        | Expr::Double(..)
        | Expr::String(..)
        | Expr::Null(..)
        | Expr::Na(..)
        | Expr::Ident { .. }
        | Expr::Unknown(..) => {}
    }
}
