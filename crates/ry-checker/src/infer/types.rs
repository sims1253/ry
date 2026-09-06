//! Type compatibility, literal shapes, and typeshed conversion.

use super::*;

pub(crate) fn equality_list_leaf_type(value: &RType) -> Option<RType> {
    if !matches!(value.mode, Mode::List) {
        return None;
    }
    let Some(schema) = value.columns.as_ref() else {
        return Some(RType::new(Mode::Opaque, value.length));
    };
    if !schema.complete {
        return Some(RType::new(Mode::Opaque, value.length));
    }
    let all_atomic = schema.columns.iter().all(|(_, leaf)| {
        matches!(
            leaf.mode,
            Mode::Logical
                | Mode::Integer
                | Mode::Double
                | Mode::Complex
                | Mode::Character
                | Mode::Raw
                | Mode::Null
        )
    });
    if !all_atomic {
        None
    } else if let Some(leaf) = schema.homogeneous_element_type() {
        Some(RType::new(leaf.mode, value.length))
    } else {
        Some(RType::new(Mode::Opaque, value.length))
    }
}

/// Wrappers that forward their first argument to an FFI primitive, so it is
/// a native routine symbol under the same convention. Unlike the primitives
/// these are ordinary R functions a user could redefine, so callers gate
/// them on `useDynLib(..., .registration = TRUE)` being declared.
///
/// `call_with_cleanup` is the cleancall wrapper vendored by purrr, cli and
/// rlang: `call_with_cleanup(map_impl, environment(), ...)`.
pub(crate) fn is_registered_ffi_wrapper(name: &str) -> bool {
    matches!(name, "call_with_cleanup")
}

/// Whether mode information permits a typed callback result. Numeric
/// conversions depend on values; opaque and union modes remain uncertain.
/// NULL's incompatibility is established separately by its zero length.
pub(crate) fn modes_compatible(mode: &Mode, target: &Mode) -> bool {
    if matches!(mode, Mode::Opaque | Mode::Union | Mode::Null) {
        return true;
    }
    match target {
        Mode::Double | Mode::Integer | Mode::Logical => numeric_family(*mode),
        Mode::Character => matches!(mode, Mode::Character),
        _ => true,
    }
}

/// Logical, integer, and double values can convert compatibly, depending
/// on their values. Mode-only checks cannot rule these conversions out.
/// Complex values are excluded because conversion can discard imaginary parts.
fn numeric_family(mode: Mode) -> bool {
    matches!(mode, Mode::Double | Mode::Integer | Mode::Logical)
}

/// Every storage mode a type may hold, unions flattened recursively.
/// Opaque participates as an ordinary mode (the permissive view used by
/// `types_intersect`); a union without a member list -- a state
/// `RType::union`'s contract forbids -- yields none.
pub(super) fn modes_of(ty: &RType) -> Vec<Mode> {
    match ty.mode {
        Mode::Union => ty
            .members
            .as_ref()
            .map(|members| members.iter().flat_map(modes_of).collect())
            .unwrap_or_default(),
        mode => vec![mode],
    }
}

/// `modes_of` restricted to fully knowable sets: `None` when the type,
/// any union member at any depth, or a member-less union is opaque.
/// `None` is a proof barrier -- callers treat it as "cannot decide",
/// never as evidence of a mismatch.
fn mode_set(ty: &RType) -> Option<Vec<Mode>> {
    let modes = modes_of(ty);
    (!modes.is_empty() && !modes.contains(&Mode::Opaque)).then_some(modes)
}

/// Return the R source symbol for a binary operator, for use in
/// diagnostic messages.
pub(crate) fn op_symbol(op: BinOpKind) -> &'static str {
    match op {
        BinOpKind::Add => "+",
        BinOpKind::Sub => "-",
        BinOpKind::Mul => "*",
        BinOpKind::Div => "/",
        BinOpKind::Pow => "^",
        BinOpKind::Mod => "%%",
        BinOpKind::IDiv => "%/%",
        BinOpKind::Colon => ":",
        BinOpKind::Lt => "<",
        BinOpKind::Le => "<=",
        BinOpKind::Gt => ">",
        BinOpKind::Ge => ">=",
        BinOpKind::Eq => "==",
        BinOpKind::Ne => "!=",
        BinOpKind::And => "&",
        BinOpKind::AndAnd => "&&",
        BinOpKind::Or => "|",
        BinOpKind::OrOr => "||",
        BinOpKind::In => "%in%",
        BinOpKind::Assign => "<-",
        BinOpKind::SuperAssign => "<<-",
        BinOpKind::PipeForward => "%>%",
        BinOpKind::PipeNative => "|>",
        BinOpKind::PipeTee => "%T>%",
        BinOpKind::PipeAssign => "%<>%",
    }
}

/// Whether `op` is one of R's six comparison operators.
pub(crate) fn is_comparison(op: BinOpKind) -> bool {
    matches!(
        op,
        BinOpKind::Lt
            | BinOpKind::Le
            | BinOpKind::Gt
            | BinOpKind::Ge
            | BinOpKind::Eq
            | BinOpKind::Ne
    )
}

/// Whether two narrowing types have a representable mode intersection.
/// This deliberately ignores length and class metadata: a guard such as
/// `is.list(x)` is about storage mode, and a default value's length/class
/// says nothing about values supplied by callers.
pub(super) fn types_intersect(left: &RType, right: &RType) -> bool {
    let left = modes_of(left);
    let right = modes_of(right);
    left.iter().any(|mode| right.contains(mode))
}

/// Result of trying to read a class literal from a `class = ...`
/// argument of `structure(...)`. `Unknown` covers dynamic expressions
/// (`class = my_var`, `class = some_call()`) which we cannot resolve at
/// compile time.
pub(crate) enum ClassLiteral {
    /// A single string literal, e.g. `class = "foo"`.
    Single(String),
    /// A `c(...)` of string literals, e.g. `class = c("foo", "bar")`.
    /// Non-string elements cause the whole vector to be reported as
    /// `Unknown` (R would coerce at runtime, but we play it safe).
    Multi(Vec<String>),
    /// Anything we can't statically read.
    Unknown,
}

/// Read a class literal from the `class = ...` argument of `structure`.
/// Recognizes `"foo"`, `c("foo")`, and `c("a", "b", ...)`. Mixed-type
/// vectors, non-literal values, and anything else become `Unknown`
/// rather than producing a wrong class.
pub(crate) fn parse_class_literal(e: &Expr) -> ClassLiteral {
    match e {
        Expr::String(s, _) => ClassLiteral::Single(s.clone()),
        Expr::Call { func, args, .. } => {
            if let Expr::Ident { name, .. } = func.as_ref() {
                if name == "c" {
                    let mut names: Vec<String> = Vec::new();
                    for a in args {
                        match &a.value {
                            Expr::String(s, _) => names.push(s.clone()),
                            _ => return ClassLiteral::Unknown,
                        }
                    }
                    if names.is_empty() {
                        return ClassLiteral::Unknown;
                    }
                    return ClassLiteral::Multi(names);
                }
            }
            ClassLiteral::Unknown
        }
        _ => ClassLiteral::Unknown,
    }
}

pub(crate) fn types_provably_incompatible(actual: &RType, expected: &RType) -> bool {
    let Some(actual_modes) = mode_set(actual) else {
        return false;
    };
    let Some(expected_modes) = mode_set(expected) else {
        return false;
    };
    !actual_modes.iter().any(|actual_mode| {
        expected_modes
            .iter()
            .any(|expected_mode| compatible_mode_pair(*actual_mode, *expected_mode))
    })
}

/// Whether every value represented by `actual` is rejected by a standalone
/// checker accepting `expected`. Unlike ordinary argument compatibility,
/// standalone checks are exact assertions: their length and class constraints
/// are runtime preconditions, and numeric modes are not interchangeable.
pub(crate) fn standalone_check_provably_rejects(actual: &RType, expected: &RType) -> bool {
    /// `modes_of`'s walk keeping full `RType`s: standalone assertions
    /// are exact, so each member's length and class are preconditions
    /// too. A member-less union degrades to the union itself.
    fn members(rtype: &RType) -> Vec<&RType> {
        if rtype.mode == Mode::Union {
            match rtype.members.as_deref() {
                Some(union_members) => union_members.iter().flat_map(|m| members(m)).collect(),
                None => vec![rtype],
            }
        } else {
            vec![rtype]
        }
    }

    fn lengths_overlap(actual: Length, expected: Length) -> bool {
        actual == Length::Unknown || expected == Length::Unknown || actual == expected
    }

    fn classes_overlap(actual: &RType, expected: &RType) -> bool {
        if !expected.class.has_known_class() {
            return true;
        }
        if actual.class.is_unknown() {
            return true;
        }
        expected
            .class
            .names
            .iter()
            .flatten()
            .any(|name| actual.class.contains(name))
    }

    fn shapes_overlap(actual: &RType, expected: &RType) -> bool {
        let modes_overlap = actual.mode == Mode::Opaque
            || expected.mode == Mode::Opaque
            || actual.mode == expected.mode;
        modes_overlap
            && lengths_overlap(actual.length, expected.length)
            && classes_overlap(actual, expected)
    }

    !members(actual).into_iter().any(|actual| {
        members(expected)
            .into_iter()
            .any(|expected| shapes_overlap(actual, expected))
    })
}

fn compatible_mode_pair(actual: Mode, expected: Mode) -> bool {
    actual == expected || (numeric_family(actual) && numeric_family(expected))
}

pub(crate) fn expected_type_label(expected: &RType) -> String {
    let Some(modes) = mode_set(expected) else {
        return "unknown".to_string();
    };
    // "numeric" is message wording, not a coercion promise: it covers
    // the wider logical..complex ladder R's own docs call numeric (see
    // `numeric_family`).
    if modes.len() >= 3
        && modes
            .iter()
            .all(|mode| numeric_family(*mode) || matches!(mode, Mode::Complex))
    {
        return "numeric".to_string();
    }
    modes
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" or ")
}

/// If `e` is a literal expression (`42`, `"x"`, `TRUE`, `NULL`, `NA`),
/// return the mode that calling it would error with.
/// Non-literal callees return `None` so the caller stays silent.
pub(crate) fn literal_callee_mode(e: &Expr) -> Option<Mode> {
    let t = infer_literal_default(e);
    (!matches!(t.mode, Mode::Opaque)).then_some(t.mode)
}

/// Compute the longest known length among a slice of argument types.
/// Used by `paste` / `paste0` / `sprintf` which return a character
/// vector whose length is the longest of the input vectors (R recycles
/// shorter args to match). Returns `Length::Unknown` if any arg has an
/// unknown length.
pub(crate) fn longest_arg_length(arg_types: &[RType]) -> Length {
    let mut max: Length = Length::One;
    for t in arg_types {
        max = match (max, t.length) {
            (Length::Zero, x) | (x, Length::Zero) => x,
            (Length::One, x) | (x, Length::One) => x,
            (Length::Known(a), Length::Known(b)) => Length::Known(a.max(b)),
            _ => return Length::Unknown,
        };
    }
    max
}

/// Build a `ColumnSchema` from a `list(...)` / `data.frame(...)` argument
/// list. Each named arg becomes a column keyed by its name; positional
/// args get R's auto-generated `[[i]]` names (1-indexed). Returns `None`
/// if there are no args at all (an empty list has no useful schema).
///
/// The arg-type vector and the arg list must be the same length; if they
/// differ (which shouldn't happen but we guard anyway) we zip by the
/// shorter one to avoid index panics.
pub(crate) fn build_named_schema(arg_types: &[RType], args: &[Arg]) -> Option<ColumnSchema> {
    if args.is_empty() {
        return None;
    }
    let mut positional = 0usize;
    let mut columns: Vec<(String, RType)> = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        let ty = arg_types.get(i).cloned().unwrap_or(RType::unknown());
        let name = match a.name.as_deref() {
            Some(n) if !n.is_empty() => semantic_argument_name(n).to_owned(),
            _ => {
                // R auto-generates `[[1]]`, `[[2]], ... for unnamed list
                // elements. We count only unnamed slots (named args do
                // not consume positional indices in R's `list()`, but
                // they do in `data.frame()`; for v1 we use a simple
                // running counter over all args, which matches the
                // common case and avoids surprising schema gaps).
                positional += 1;
                format!("[[{}]]", positional)
            }
        };
        columns.push((name, ty));
    }
    Some(ColumnSchema {
        columns,
        complete: true,
        locally_constructed: false,
    })
}

/// `data.frame()` derives names for simple positional expressions from the
/// expression itself (`data.frame(y, K)` has columns `y` and `K`). Lists do
/// not: their unnamed elements retain positional placeholders. Keep the two
/// constructor rules separate so improving data-frame fidelity cannot change
/// list indexing semantics.
pub(crate) fn build_data_frame_schema(arg_types: &[RType], args: &[Arg]) -> Option<ColumnSchema> {
    let mut schema = build_named_schema(arg_types, args)?;
    debug_assert_eq!(schema.columns.len(), args.len());
    for ((name, _), arg) in schema.columns.iter_mut().zip(args) {
        if arg.name.is_none() {
            let Expr::Ident { name: symbol, .. } = &arg.value else {
                // Unlike list placeholders, `[[i]]` is not a reliable
                // data-frame column name. If an expression's resulting names
                // are unknown, keep the whole schema opaque so a fabricated
                // name can never justify RY060.
                return None;
            };
            *name = symbol.clone();
        }
    }
    Some(schema)
}

pub(crate) fn semantic_argument_name(name: &str) -> &str {
    if name.len() >= 2 {
        let bytes = name.as_bytes();
        let quoted = matches!(
            (bytes[0], bytes[name.len() - 1]),
            (b'"', b'"') | (b'\'', b'\'') | (b'`', b'`')
        );
        if quoted {
            return &name[1..name.len() - 1];
        }
    }
    name
}

/// Convert a dataset type and its immediate column types from the typeshed.
pub(crate) fn json_rtype_to_rtype(jt: &JsonRType) -> RType {
    let base = json_rtype_scalar(jt);
    if jt.columns.is_empty() {
        return base;
    }
    // Build the column schema. We recurse via a single-level helper so
    // a dataset's `columns.<col>.columns` (which is empty in practice)
    // does not trigger further nesting.
    let cols: Vec<(String, RType)> = jt
        .columns
        .iter()
        .map(|(name, child)| (name.clone(), json_rtype_scalar(child)))
        .collect();
    let schema = Arc::new(ColumnSchema {
        columns: cols,
        complete: true,
        locally_constructed: false,
    });
    base.with_columns(schema)
}

/// Map a parsed `JsonLength` to the checker's `Length`. Literal lengths
/// map exactly; every arg-derived spec (`arg0`, `longest_arg`, ...) and a
/// missing spec map to `Length::Unknown`, so callers that resolve those
/// specs contextually must do so before falling back to this.
pub(crate) fn json_length_to_length(spec: Option<JsonLength>) -> Length {
    match spec {
        Some(JsonLength::Known(0)) => Length::Zero,
        Some(JsonLength::Known(1)) => Length::One,
        Some(JsonLength::Known(value)) => Length::Known(value),
        _ => Length::Unknown,
    }
}

pub(crate) fn json_rtype_scalar(jt: &JsonRType) -> RType {
    let length = json_length_to_length(JsonLength::parse(&jt.length));
    if matches!(JsonMode::parse(&jt.mode), Some(JsonMode::Union)) {
        let members: Vec<RType> = jt
            .members
            .iter()
            .filter_map(|member| concrete_json_mode(member))
            .map(|mode| RType::new(mode, length))
            .collect();
        return if members.is_empty() {
            RType::unknown()
        } else {
            RType::union(Arc::from(members))
        };
    }
    let mode = concrete_json_mode(&jt.mode).unwrap_or(Mode::Opaque);
    let class = if jt.class.is_empty() {
        ClassVector::empty()
    } else {
        let refs: Vec<&str> = jt.class.iter().map(|s| s.as_str()).collect();
        ClassVector::from_slice(&refs)
    };
    RType::new(mode, length).with_class(class)
}

/// Map a typeshed mode string to the concrete `Mode` it names. Returns
/// `None` for `union`, the compound arg-derived specs, and unrecognized
/// strings; callers decide their own fallback for those.
pub(crate) fn concrete_json_mode(mode: &str) -> Option<Mode> {
    Some(match JsonMode::parse(mode)? {
        JsonMode::Logical => Mode::Logical,
        JsonMode::Integer => Mode::Integer,
        JsonMode::Double => Mode::Double,
        JsonMode::Character => Mode::Character,
        JsonMode::Complex => Mode::Complex,
        JsonMode::Raw => Mode::Raw,
        JsonMode::List => Mode::List,
        JsonMode::Null => Mode::Null,
        JsonMode::Function => Mode::Function,
        JsonMode::Opaque => Mode::Opaque,
        _ => return None,
    })
}

/// Pins for the mode/union-set helpers unified in issue #169:
/// `modes_compatible`, `types_intersect`,
/// `types_provably_incompatible`/`expected_type_label`,
/// `standalone_check_provably_rejects`, and the narrowing call sites.
/// Each row is a decision a call site relies on.
#[cfg(test)]
mod mode_union_set_pins {
    use super::*;

    /// Test-only union of mode-only scalars (length One, no class); must not be used where length/class matter (the standalone pins keep bespoke constructions).
    fn union(modes: &[Mode]) -> RType {
        RType::union(Arc::from_iter(
            modes.iter().map(|mode| RType::scalar(*mode)),
        ))
    }

    #[test]
    fn modes_compatible_pins_the_coercion_view() {
        for (actual, target, compatible) in [
            // No evidence: opaque/union/null callback returns stay compatible.
            (Mode::Opaque, Mode::Double, true),
            (Mode::Union, Mode::Double, true),
            (Mode::Null, Mode::Double, true),
            // The silent-coercion family interchanges freely.
            (Mode::Logical, Mode::Double, true),
            (Mode::Integer, Mode::Logical, true),
            (Mode::Double, Mode::Integer, true),
            // The footguns RY080 exists for.
            (Mode::Character, Mode::Double, false),
            (Mode::Double, Mode::Character, false),
            // Complex into a numeric target discards imaginary parts with
            // a warning, so it is not silently compatible; unmodeled
            // targets stay permissive.
            (Mode::Complex, Mode::Double, false),
            (Mode::Double, Mode::Complex, true),
        ] {
            assert_eq!(
                modes_compatible(&actual, &target),
                compatible,
                "{actual:?} into {target:?}"
            );
        }
    }

    #[test]
    fn types_intersect_pins_mode_overlap() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        let numeric_union = union(&[Mode::Integer, Mode::Double]);
        let opaque_member_union = union(&[Mode::Character, Mode::Opaque]);
        for (left, right, overlap) in [
            (&double, &double, true),
            (&character, &double, false),
            (&numeric_union, &double, true),
            (&double, &numeric_union, true),
            (&numeric_union, &character, false),
            // Opaque is a mode matching only itself: opaque-vs-double is
            // disjoint -- what lets a guarded default parameter be replaced
            // by the predicate's type -- and an opaque-bearing union still
            // intersects through its knowable members.
            (&RType::unknown(), &double, false),
            (&double, &RType::unknown(), false),
            (&RType::unknown(), &RType::unknown(), true),
            (&opaque_member_union, &double, false),
            (&opaque_member_union, &character, true),
        ] {
            assert_eq!(
                types_intersect(left, right),
                overlap,
                "{left:?} vs {right:?}"
            );
        }
    }

    #[test]
    fn types_provably_incompatible_pins_argument_checking() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        for (actual, expected, incompatible) in [
            (&character, &double, true),
            (&RType::scalar(Mode::Integer), &double, false),
            (&double, &RType::scalar(Mode::Logical), false),
            // Complex is outside the silent-coercion family.
            (&RType::scalar(Mode::Complex), &double, true),
            // Opaque anywhere means "cannot decide", never a mismatch.
            (&RType::unknown(), &double, false),
            (&double, &RType::unknown(), false),
            (&union(&[Mode::Double, Mode::Opaque]), &character, false),
            // A union with any compatible member passes.
            (&union(&[Mode::Character, Mode::Double]), &double, false),
            (&RType::scalar(Mode::List), &double, true),
        ] {
            assert_eq!(
                types_provably_incompatible(actual, expected),
                incompatible,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    #[test]
    fn expected_type_label_pins_message_text() {
        for (expected, label) in [
            (&RType::scalar(Mode::Double), "double"),
            (&RType::unknown(), "unknown"),
            (
                &union(&[Mode::Logical, Mode::Integer, Mode::Double]),
                "numeric",
            ),
            // The label's one deliberate divergence from `numeric_family`:
            // "numeric" is wording for unions R's own documentation calls
            // numeric, so complex is included.
            (
                &union(&[Mode::Integer, Mode::Double, Mode::Complex]),
                "numeric",
            ),
        ] {
            assert_eq!(expected_type_label(expected), label, "{expected:?}");
        }
    }

    #[test]
    fn standalone_check_pins_exact_assertions() {
        let double = RType::scalar(Mode::Double);
        let character = RType::scalar(Mode::Character);
        let nullable_character = RType::union(Arc::from(vec![
            RType::new(Mode::Null, Length::Zero),
            character.clone(),
        ]));
        for (actual, expected, rejects) in [
            // Exact assertions: numeric modes are not interchangeable
            // (unlike ordinary argument compatibility), and length is a
            // runtime precondition. Opaque never rejects.
            (&character, &double, true),
            (&double, &double, false),
            (&RType::scalar(Mode::Integer), &double, true),
            (&double, &RType::new(Mode::Double, Length::Known(2)), true),
            (&double, &RType::new(Mode::Double, Length::Unknown), false),
            (&RType::unknown(), &double, false),
            // A union expectation with any overlapping member passes.
            (&character, &nullable_character, false),
            (&double, &nullable_character, true),
        ] {
            assert_eq!(
                standalone_check_provably_rejects(actual, expected),
                rejects,
                "{actual:?} vs {expected:?}"
            );
        }
    }

    // Pins installed-narrowing outcomes rather than the compatibility
    // flag itself: the opaque-default arms are disjoint from the guard
    // (not merely "unprovable"), pinning the disjoint-default install
    // path, and a plain union binding narrows to the confirmed member.
    #[test]
    fn guards_install_over_opaque_defaults_and_confirmed_members() {
        let target = RType::scalar(Mode::Double);
        for (as_default, existing) in [
            (true, RType::unknown()),
            (true, union(&[Mode::Character, Mode::Opaque])),
            (false, union(&[Mode::Integer, Mode::Double])),
        ] {
            // Discriminate the disjointness claim: the opaque-default arms
            // are disjoint from the guard (opaque matches only itself),
            // while the confirmed-member arm overlaps through Double.
            if as_default {
                assert!(
                    !types_intersect(&existing, &target),
                    "opaque default must be disjoint from the guard: {existing:?}"
                );
            } else {
                assert!(
                    types_intersect(&existing, &target),
                    "confirmed-member union must overlap the guard: {existing:?}"
                );
            }
            let note = format!("guard must install its type over {existing:?}");
            let mut base = Scope::default();
            if as_default {
                base.insert_parameter_default("x", existing);
            } else {
                base.insert("x", existing);
            }
            let (then_scope, ..) = apply_narrowing(
                &base,
                &Narrowing::Positive {
                    var: "x".to_string(),
                    target: target.clone(),
                },
            );
            assert_eq!(
                then_scope.get("x").map(|t| t.mode),
                Some(Mode::Double),
                "{note}"
            );
        }
    }
}
