//! Static R package metadata extracted from R source and NAMESPACE files.
//!
//! This module establishes whether package-provided names are in scope. It
//! intentionally does not assign precise types: imported or attached exports
//! become opaque bindings unless an embedded typeshed can refine them.

use ry_core::SourceFile;
use ry_core::ast::{Expr, Stmt};
use ry_core::walk::{AstNode, Descend, Walk, walk_stmts};
use ry_typeshed::Typeshed;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;

/// Per-file package facts that are not R bindings.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PackageFacts {
    /// DESCRIPTION's Package name for a file in the package's R sources.
    /// Tests and scripts do not install their definitions in that namespace.
    pub source_package: Option<String>,
    /// `useDynLib(..., .fixes = "prefix")` prefixes. Any name starting with
    /// one resolves to a native routine.
    pub native_routine_prefixes: HashSet<String>,
    /// `useDynLib(..., .registration = TRUE)`. The registered entry points are
    /// declared in `src/`'s `R_registerRoutines` table; this flag also
    /// retains call-position support when source tables cannot be enumerated.
    pub native_registration: bool,
    /// Package `R/` code looks up its namespace, its imports, then base, and
    /// only then the search path, so attached packages cannot mask base
    /// there. Set only without whole-package `import()`s: an installed export
    /// list does not prove an import lacks a base name.
    pub base_before_search_path: bool,
    /// Class and function names each fully read `RCPP_MODULE` in `src/`
    /// declares, for `Rcpp::loadModule(module, TRUE)`. Read only for a
    /// package whose R code calls `loadModule()`.
    pub rcpp_modules: Arc<BTreeMap<String, BTreeSet<String>>>,
}

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
    /// Literal DLL names and binding prefixes with registration enabled.
    pub registered_native_libraries: Vec<(String, String)>,
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
    /// Whether `exportPattern()` may make names public beyond `exports`.
    pub export_patterns: bool,
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
                let registered = args.iter().any(|arg| {
                    arg.name.as_deref() == Some(".registration")
                        && matches!(&arg.value, Expr::Logical(true, _))
                });
                metadata.native_registration |= registered;
                if registered
                    && let Some(library) = args.first().and_then(|arg| static_name(&arg.value))
                {
                    let prefix = args
                        .iter()
                        .find(|arg| arg.name.as_deref() == Some(".fixes"));
                    if let Some(prefix) =
                        prefix.map_or(Some(String::new()), |arg| static_name(&arg.value))
                    {
                        metadata.registered_native_libraries.push((library, prefix));
                    }
                }
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
            "exportPattern" => metadata.export_patterns = true,
            "S3method" => {
                let generic = args.first().and_then(|arg| static_name(&arg.value));
                if let (Some(generic), Some(class)) = (
                    &generic,
                    args.get(1).and_then(|arg| static_name(&arg.value)),
                ) {
                    // `S3method(pkg::generic, class)` registers a method
                    // for the bare generic that calls dispatch through.
                    let generic = generic
                        .rsplit_once("::")
                        .map_or(generic.as_str(), |(_, bare)| bare);
                    metadata.s3_methods.insert((generic.to_string(), class));
                }
            }
            _ => {}
        }
    }
    metadata
}

/// `package` followed by the packages attaching it also attaches: a
/// meta-package's declared `attaches`, read from a user stub when one
/// replaces the embedded stub.
pub fn attaching<'a>(
    package: &'a str,
    user_stubs: &'a BTreeMap<String, Typeshed>,
) -> impl Iterator<Item = &'a str> {
    let attaches = ry_typeshed::package_stub(user_stubs, package)
        .map_or(&[][..], |stub| stub.attaches.as_slice());
    std::iter::once(package).chain(attaches.iter().map(String::as_str))
}

/// `packages` together with everything they attach (see [`attaching`]).
pub fn with_attached(
    packages: HashSet<String>,
    user_stubs: &BTreeMap<String, Typeshed>,
) -> HashSet<String> {
    packages
        .iter()
        .flat_map(|package| attaching(package, user_stubs))
        .map(str::to_owned)
        .collect()
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

    fn assert_exact(src: &str, expected: &[&str]) {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("attach_test.R", src).unwrap();
        let expected: HashSet<String> = expected.iter().map(|name| name.to_string()).collect();
        assert_eq!(attached_packages(&file), expected, "attached from `{src}`");
    }

    /// An attachment counts wherever the call appears: the walker uses
    /// `Walk::ALL`, so `library()`/`require()` calls nested inside
    /// function bodies (an `.onLoad` hook, a helper defined for tests)
    /// attach their package just like top-level ones.
    #[test]
    fn attaches_calls_at_top_level_and_inside_function_bodies() {
        assert_exact(
            "library(dplyr)
run <- function() {
  require(\"stringr\")
  if (verbose) library(rlang)
}
",
            &["dplyr", "stringr", "rlang"],
        );
    }

    /// `requireNamespace()`/`loadNamespace()` make `pkg::name` available
    /// without placing exports on the search path, and a package name
    /// computed at runtime is not statically knowable. None of these
    /// attach anything.
    #[test]
    fn does_not_attach_namespace_helpers_or_dynamic_arguments() {
        assert_exact(
            "requireNamespace(\"rlang\")
loadNamespace(\"tibble\")
library(get_option(\"pkg\"))
my_library(dplyr)
",
            &[],
        );
    }

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
    fn qualified_s3_registrations_key_the_bare_generic() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser
            .parse(
                "NAMESPACE",
                "S3method(tidyr::expand, tbl_lazy)\nS3method(print, tbl_lazy)\n",
            )
            .unwrap();
        assert_eq!(
            namespace_metadata(&file).s3_methods,
            HashSet::from([
                ("expand".to_string(), "tbl_lazy".to_string()),
                ("print".to_string(), "tbl_lazy".to_string()),
            ])
        );
    }

    #[test]
    fn registered_libraries_keep_their_own_prefixes() {
        let mut parser = ry_core::RParser::new().unwrap();
        let file = parser.parse("NAMESPACE", "useDynLib(example, .registration = TRUE)\nuseDynLib(other, .registration = TRUE, .fixes = 'C_')").unwrap();
        assert_eq!(
            namespace_metadata(&file).registered_native_libraries,
            vec![("example".into(), "".into()), ("other".into(), "C_".into())]
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
