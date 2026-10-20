//! Known environment and language values built by base constructors.

use super::*;
use crate::resolve::SpecialCallProvenance;

impl Checker {
    /// The value kind a base constructor establishes from syntax alone:
    /// `quote(<symbol or call>)` and `y ~ x` build language objects, and
    /// `ENVIRONMENT_CONSTRUCTORS` build environments. Classified before the
    /// call runs, because R fixes the callee before forcing promises.
    ///
    /// `None` keeps the ordinary call result. Only a proven base callee
    /// counts: masked heads, aliases, an uncertain search path, data masks
    /// and user base stubs prove nothing.
    pub(super) fn base_value_kind(
        &self,
        func: &Expr,
        args: &[Arg],
        scope: &Scope,
    ) -> Option<RType> {
        let name = call::callee_name(func)?;
        let lookup_name = crate::semantic_lists::bare_name(&name);
        let kind = match lookup_name {
            "quote" => quoted_language(args)?,
            // The "formula" class is not established yet: it would open S3
            // dispatch to every `*.formula` method.
            "~" => RType::new(Mode::Language, Length::Nonempty).with_class(ClassVector::unknown()),
            // Attributes are shared by reference, so any function the
            // environment reaches may give it a class.
            _ if crate::semantic_lists::ENVIRONMENT_CONSTRUCTORS.contains(&lookup_name) => {
                RType::new(Mode::Environment, Length::Unknown).with_class(ClassVector::unknown())
            }
            _ => return None,
        };
        let semantic_name = scope.function_alias(&name).unwrap_or(&name);
        matches!(
            self.special_call_provenance(&name, func, semantic_name, lookup_name, "base", scope),
            SpecialCallProvenance::Proven
        )
        .then_some(kind)
    }
}

/// `quote(expr)` of a symbol or call: a fresh, classless language object
/// (symbols cannot carry attributes). Literals stay with the stub type,
/// since `quote(1)` is the number itself.
fn quoted_language(args: &[Arg]) -> Option<RType> {
    let [arg] = args else {
        return None;
    };
    if arg.name.as_deref().is_some_and(|name| name != "expr") {
        return None;
    }
    let length = match &arg.value {
        // The parser drops `(` and unary `+`, which build calls: only an
        // argument spelled as the bare symbol is proven length one.
        Expr::Ident { span, .. } if arg.name.is_none() && arg.span == *span => Length::One,
        Expr::Ident { .. }
        | Expr::Call { .. }
        | Expr::BinOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Index { .. }
        | Expr::Function { .. }
        | Expr::If { .. }
        | Expr::Block { .. } => Length::Nonempty,
        _ => return None,
    };
    Some(RType::new(Mode::Language, length))
}
