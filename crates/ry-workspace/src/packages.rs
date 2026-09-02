//! Static R package metadata extracted from R source and NAMESPACE files.
//!
//! This module establishes whether package-provided names are in scope. It
//! intentionally does not assign precise types: imported or attached exports
//! become opaque bindings unless an embedded typeshed can refine them.

use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;

/// External-binding sentinel carrying a `useDynLib(..., .fixes = "prefix")`
/// prefix. Any name starting with the prefix resolves to a native routine.
/// The `\0` keeps it out of the R identifier namespace.
pub const NATIVE_ROUTINE_PREFIX_SENTINEL: &str = "\0useDynLib:";

/// External-binding sentinel recording `useDynLib(..., .registration = TRUE)`.
/// The registered entry points are declared in `src/`'s `R_registerRoutines`
/// table, which ry does not read, so the flag instead licenses bare symbols in
/// native-call argument position.
pub const NATIVE_REGISTRATION_SENTINEL: &str = "\0useDynLibRegistration";

/// Bindings and whole-package imports declared by an R package NAMESPACE.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct NamespaceMetadata {
    /// Native routine prefixes introduced by `useDynLib(..., .fixes = "prefix")`.
    pub native_routine_prefixes: HashSet<String>,
    /// Native routine names explicitly listed by `useDynLib(pkg, foo, bar)`.
    pub native_routines: HashSet<String>,
    /// Whether `useDynLib(..., .registration = TRUE)` enables registered
    /// symbols in native-call positions without enumerating C sources.
    pub native_registration: bool,
    /// Names introduced by `importFrom(package, name, ...)`.
    pub imported_bindings: HashSet<String>,
    /// Exact package provenance for `importFrom(package, name, ...)` names.
    /// This lets metadata be applied to that binding without attaching every
    /// other export from the package.
    pub imported_from: HashMap<String, String>,
    /// Packages introduced by `import(package, ...)`.
    pub imported_packages: HashSet<String>,
    /// Names made public by `export(name, ...)`.
    pub exports: HashSet<String>,
    /// Generic names mentioned by `S3method(generic, class)`. A generic is
    /// looked up in function position even when a data binding with the same
    /// name exists locally, so these are function candidates as well as
    /// namespace metadata.
    pub s3_generics: HashSet<String>,
    /// Explicit `(generic, class)` registrations from `S3method()`.
    pub s3_methods: HashSet<(String, String)>,
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
            "useDynLib" => {
                metadata.native_routine_prefixes.extend(
                    args.iter()
                        .filter(|arg| arg.name.as_deref() == Some(".fixes"))
                        .filter_map(|arg| static_name(&arg.value))
                        .filter(|prefix| !prefix.is_empty()),
                );
                metadata.native_registration |= args.iter().any(|arg| {
                    arg.name.as_deref() == Some(".registration")
                        && matches!(&arg.value, Expr::Logical(true, _))
                });
                metadata.native_routines.extend(
                    args.iter()
                        .skip(1)
                        .filter(|arg| arg.name.is_none())
                        .filter_map(|arg| static_name(&arg.value)),
                );
            }
            "importFrom" => {
                if let Some(package) = args.first().and_then(|arg| static_name(&arg.value)) {
                    for binding in args
                        .iter()
                        .skip(1)
                        .filter_map(|arg| static_name(&arg.value))
                    {
                        metadata.imported_bindings.insert(binding.clone());
                        metadata.imported_from.insert(binding, package.clone());
                    }
                }
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
            "S3method" => {
                let generic = args.first().and_then(|arg| static_name(&arg.value));
                if let Some(generic) = &generic {
                    metadata.s3_generics.insert(generic.clone());
                }
                if let (Some(generic), Some(class)) = (
                    &generic,
                    args.get(1).and_then(|arg| static_name(&arg.value)),
                ) {
                    metadata.s3_methods.insert((generic.clone(), class));
                }
            }
            _ => {}
        }
    }
    metadata
}

/// Find packages attached by `library()` or `require()` calls.
///
/// `requireNamespace()` is deliberately excluded: it makes `pkg::name`
/// available but does not place `name` on R's search path. Walks every
/// subtree including function bodies: an attachment counts wherever the
/// call appears.
pub fn attached_packages(file: &SourceFile) -> HashSet<String> {
    let mut packages = HashSet::new();
    let _ = walk_stmts(&file.stmts, Walk::ALL, |node: AstNode<'_>, _: usize| {
        if let AstNode::Expr(Expr::Call { func, args, .. }) = node
            && matches!(func.as_ref(), Expr::Ident { name, .. } if name == "library" || name == "require")
            && let Some(package) = args.first().and_then(|arg| static_name(&arg.value))
        {
            packages.insert(package);
        }
        ControlFlow::<(), Descend>::Continue(Descend::Into)
    });
    packages
}

fn static_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_from_preserves_binding_provenance_without_attaching_package() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse("NAMESPACE", "importFrom(dplyr, select, mutate)")
            .unwrap();
        let metadata = namespace_metadata(&file);

        assert!(metadata.imported_packages.is_empty());
        assert_eq!(
            metadata.imported_from.get("select").map(String::as_str),
            Some("dplyr")
        );
        assert_eq!(
            metadata.imported_from.get("mutate").map(String::as_str),
            Some("dplyr")
        );
    }

    #[test]
    fn use_dyn_lib_records_registration_and_explicit_routines() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "NAMESPACE",
                "useDynLib(pkg, foo, bar, .registration = TRUE, .fixes = \"pkg_\")\n",
            )
            .unwrap();
        let metadata = namespace_metadata(&file);
        assert!(metadata.native_registration);
        assert_eq!(
            metadata.native_routines,
            HashSet::from(["foo".to_string(), "bar".to_string()])
        );
        assert_eq!(
            metadata.native_routine_prefixes,
            HashSet::from(["pkg_".to_string()])
        );
    }

    #[test]
    fn use_dyn_lib_records_nonempty_fixes_prefixes() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "NAMESPACE",
                "useDynLib(pkg, .fixes = \"pkg_\")\nuseDynLib(other)\nuseDynLib(empty, .fixes = \"\")",
            )
            .unwrap();

        assert_eq!(
            namespace_metadata(&file).native_routine_prefixes,
            HashSet::from(["pkg_".to_string()])
        );
    }
}
