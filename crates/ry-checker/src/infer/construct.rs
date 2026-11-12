use super::*;
use crate::resolve::SpecialCallProvenance;

impl Checker {
    /// The class-constructor stage of `infer_call`: `structure(x, class =
    /// ...)`, `factor(x)`, and S4 `new("Class", ...)` attach a class to a
    /// payload value.
    pub(crate) fn infer_class_constructor_call(
        &mut self,
        original_name: &str,
        original_callee: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        // `structure(x, class = "...")` is R's class constructor. We
        // model only the common literal forms:
        //   * `class = "foo"` attaches a single class.
        //   * `class = c("a", "b", ...)` attaches a class vector.
        // Non-literal or unparseable forms fall through to opaque
        // inference with `ClassVector::unknown()` so RY050 stays quiet.
        if lookup_name == "structure" {
            match self.structure_provenance(original_name, original_callee, semantic_name, scope) {
                SpecialCallProvenance::Proven => {
                    return Some(self.infer_structure_call(args, scope));
                }
                SpecialCallProvenance::Ordinary => {}
                // Do not borrow base-stub payload facts or force quoted
                // arguments.
                SpecialCallProvenance::Unknown | SpecialCallProvenance::AmbientUncertainty => {
                    return Some(RType::unknown());
                }
            }
        }
        if matches!(lookup_name, "factor" | "new") {
            let package = if lookup_name == "factor" {
                "base"
            } else {
                "methods"
            };
            match self.special_call_provenance(
                original_name,
                original_callee,
                semantic_name,
                lookup_name,
                package,
                scope,
            ) {
                SpecialCallProvenance::Proven => {}
                SpecialCallProvenance::Ordinary => return None,
                SpecialCallProvenance::Unknown | SpecialCallProvenance::AmbientUncertainty => {
                    return Some(Self::infer_unknown_constructor(scope));
                }
            }
        }
        // `factor(x)` returns an integer vector with class "factor".
        // (And often also "ordered" if `ordered = TRUE`, but we keep v1
        // to the base case.)
        if lookup_name == "factor" {
            // Infer args so unbound-variable diagnostics still fire.
            self.infer_args_for_diagnostics(args, scope);
            return Some(
                RType::new(Mode::Integer, Length::Unknown)
                    .with_class(ClassVector::single("factor")),
            );
        }
        if lookup_name == "new" {
            return Some(self.infer_methods_new(args, scope));
        }
        None
    }

    fn infer_unknown_constructor(scope: &mut Scope) -> RType {
        scope.invalidate_unknown_effects();
        RType::unknown()
    }

    fn infer_methods_new(&mut self, args: &[Arg], scope: &mut Scope) -> RType {
        let matched = match_argument_names(
            &["Class", "..."],
            args.iter()
                .map(|arg| arg.name.as_deref().map(semantic_argument_name)),
        );
        let Some(class_index) = matched.arg_for_param(0) else {
            return Self::infer_unknown_constructor(scope);
        };
        let exact = args
            .iter()
            .filter(|arg| arg.name.as_deref().map(semantic_argument_name) == Some("Class"))
            .count();
        let partial = args
            .iter()
            .filter(|arg| {
                arg.name
                    .as_deref()
                    .map(semantic_argument_name)
                    .is_some_and(|name| !name.is_empty() && "Class".starts_with(name))
            })
            .count();
        if exact > 1
            || (exact == 0 && partial > 1)
            || args.iter().any(|arg| {
                matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                    || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_))
            })
        {
            return Self::infer_unknown_constructor(scope);
        }
        // Class is forced before initialization. Retain the existing dots
        // traversal until initializer laziness has a complete effect model.
        self.infer(&args[class_index].value, scope);
        for (index, argument) in args.iter().enumerate() {
            if index != class_index {
                self.infer(&argument.value, scope);
            }
        }
        match &args[class_index].value {
            Expr::String(class, _)
                if !class.is_empty()
                    && !crate::semantic_lists::PLAIN_NEW_CLASSES.contains(&class.as_str()) =>
            {
                // S3 dispatch and `inherits()` also see an S4 instance's
                // superclasses, which its class attribute does not list.
                RType::unknown().with_class(ClassVector::guard(class))
            }
            _ => RType::unknown(),
        }
    }

    /// Preserve the `.Data` payload under a literal class attachment. Match
    /// the real formal before inspecting attributes, so an opaque payload
    /// cannot be replaced by a later positional argument.
    pub(crate) fn infer_structure_call(&mut self, args: &[Arg], scope: &mut Scope) -> RType {
        let matched = match_argument_names(
            &[".Data", "..."],
            args.iter()
                .map(|arg| arg.name.as_deref().map(semantic_argument_name)),
        );
        let Some(payload) = matched.arg_for_param(0) else {
            self.infer_args_for_diagnostics(args, scope);
            return RType::unknown();
        };
        let exact_payloads = args
            .iter()
            .filter(|arg| arg.name.as_deref().map(semantic_argument_name) == Some(".Data"))
            .count();
        let partial_payloads = args
            .iter()
            .filter(|arg| {
                arg.name
                    .as_deref()
                    .map(semantic_argument_name)
                    .is_some_and(|name| !name.is_empty() && ".Data".starts_with(name))
            })
            .count();
        // The matcher tolerates duplicate names for ordinary diagnostics;
        // constructor facts must not pick one side of an invalid binding.
        if exact_payloads > 1
            || (exact_payloads == 0 && partial_payloads > 1)
            || args.iter().enumerate().any(|(index, arg)| {
                matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                    || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_))
                    || (index != payload && arg.name.is_none())
            })
        {
            self.infer_args_for_diagnostics(args, scope);
            return RType::unknown();
        }
        // R forces .Data before constructing list(...), even when its named
        // actual occurs after attributes in source order.
        let mut base_type = self.infer(&args[payload].value, scope);
        if base_type.mode == Mode::Null {
            return RType::unknown();
        }
        let mut class_literal = None;
        let mut clear_class = false;
        let mut repeated_class = false;
        for (index, arg) in args.iter().enumerate() {
            if index == payload {
                continue;
            }
            match arg.name.as_deref().map(semantic_argument_name) {
                Some("names" | ".Names") => base_type.columns = None,
                Some("class") => {
                    repeated_class |= class_literal.is_some();
                    // Resolve c before evaluating this argument can change
                    // the scope, but after earlier attributes have run.
                    class_literal = Some(self.structure_class_literal(&arg.value, scope));
                    clear_class = matches!(&arg.value, Expr::Null(_));
                }
                _ => {}
            }
            self.infer(&arg.value, scope);
        }
        if repeated_class {
            return RType::unknown();
        }
        if clear_class {
            return base_type.with_class(ClassVector::empty());
        }
        if let Some(class_literal) = class_literal {
            let class = match class_literal {
                ClassLiteral::Single(name) => ClassVector::single(&name),
                ClassLiteral::Multi(names) => {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    ClassVector::from_slice(&refs)
                }
                ClassLiteral::Unknown => ClassVector::unknown(),
            };
            if class.contains("factor") && base_type.mode == Mode::Double {
                base_type.mode = Mode::Integer;
            }
            return base_type.with_class(class);
        }
        base_type
    }

    pub(crate) fn structure_class_literal(&self, expr: &Expr, scope: &Scope) -> ClassLiteral {
        let Expr::Call { func, args, .. } = expr else {
            return parse_class_literal(expr);
        };
        let Some(name) = ident_name(func) else {
            return ClassLiteral::Unknown;
        };
        if crate::semantic_lists::bare_name(name) != "c"
            || !self.resolves_to_base(name, scope)
            || self.structure_namespace_unavailable(name, scope)
            || self.structure_bare_callee_unavailable(name, scope)
            || args.iter().any(|arg| arg.name.is_some())
        {
            return ClassLiteral::Unknown;
        }
        let names: Option<Vec<_>> = args
            .iter()
            .map(|arg| match &arg.value {
                Expr::String(name, _) => Some(name.clone()),
                _ => None,
            })
            .collect();
        match names {
            Some(names) if !names.is_empty() => ClassLiteral::Multi(names),
            _ => ClassLiteral::Unknown,
        }
    }

    /// Whether `structure` is base's class constructor. Unlike
    /// [`Self::special_call_provenance`], a parameter or another binding
    /// that hides base leaves the call on the ordinary path.
    fn structure_provenance(
        &self,
        original_name: &str,
        original_callee: &Expr,
        semantic_name: &str,
        scope: &Scope,
    ) -> SpecialCallProvenance {
        // Spelling aliases and an unavailable namespace do not prove which
        // function object was captured.
        if original_name != semantic_name
            || (matches!(original_callee, Expr::String(_, _)) && semantic_name.contains("::"))
            || self.structure_namespace_unavailable(semantic_name, scope)
        {
            return SpecialCallProvenance::Unknown;
        }
        if self.user_stubs.contains_key("base")
            || !self.resolves_to_base_lenient(semantic_name, scope)
        {
            return SpecialCallProvenance::Ordinary;
        }
        // An unproven binding or search path may supply another callable.
        if self.resolves_to_base(semantic_name, scope)
            && !self.structure_bare_callee_unavailable(semantic_name, scope)
        {
            SpecialCallProvenance::Proven
        } else {
            SpecialCallProvenance::Unknown
        }
    }

    fn structure_bare_callee_unavailable(&self, name: &str, scope: &Scope) -> bool {
        if name.contains("::") {
            return false;
        }
        // Pooled names can denote callable values without a function-table
        // entry. Require an unshadowed spelling, including backticks and any
        // undecoded escapes, rather than assume that such values are data.
        scope.data_mask_unknown
            || self.literal_bindings_may_be_shadowed(
                [name, format!("`{name}`").as_str()],
                &HashSet::new(),
                scope,
            )
    }

    fn structure_namespace_unavailable(&self, name: &str, scope: &Scope) -> bool {
        crate::semantic_lists::is_base_qualified(name)
            && self.literal_bindings_may_be_shadowed(
                ["::", "`::`", ":::", "`:::`"],
                &HashSet::new(),
                scope,
            )
    }

    /// The read-only half of [`Self::infer_class_constructor_call`]:
    /// whether this direct call provably attaches a *known* class to its
    /// result, and therefore establishes that S3/S4 method dispatch can
    /// replace what the base function would have computed. Consumers are
    /// applicability gates that run before operand inference collapses
    /// the call to a type (RY107's dispatch boundary); they cannot invoke
    /// the constructor stage itself, which infers arguments and mutates
    /// the scope, so this predicate mirrors that stage's resolution gates
    /// and argument-shape guards with a conservative bias: every case it
    /// cannot decide reads as "not established" and leaves the caller's
    /// default heuristic in force.
    pub(crate) fn class_constructor_call_establishes_dispatch(
        &self,
        original_name: &str,
        original_callee: &Expr,
        semantic_name: &str,
        lookup_name: &str,
        args: &[Arg],
        scope: &Scope,
    ) -> bool {
        if lookup_name == "structure" {
            return matches!(
                self.structure_provenance(original_name, original_callee, semantic_name, scope),
                SpecialCallProvenance::Proven
            ) && self.structure_args_attach_known_class(args, scope);
        }
        if matches!(lookup_name, "factor" | "new") {
            let package = if lookup_name == "factor" {
                "base"
            } else {
                "methods"
            };
            // Only a proven base `factor` / methods `new` is a constructor
            // here. `Ordinary` resolves to some other function whose result
            // the constructor stage never modeled, and `Unknown`/
            // `AmbientUncertainty` degrade to an unknown class -- none of
            // them establishes dispatch.
            if !matches!(
                self.special_call_provenance(
                    original_name,
                    original_callee,
                    semantic_name,
                    lookup_name,
                    package,
                    scope,
                ),
                SpecialCallProvenance::Proven
            ) {
                return false;
            }
            if lookup_name == "factor" {
                // The base constructor always attaches class "factor".
                return true;
            }
            return self.methods_new_args_attach_known_class(args);
        }
        false
    }

    /// Whether a proven base `structure(...)` call attaches a *known*
    /// class. Mirrors the argument-shape guards of
    /// [`Self::infer_structure_call`] -- the payload/`.Data` binding, the
    /// `...`/parser-opaque splice forms, and the duplicate-binding
    /// tolerance the constructor stage refuses to pick a side of -- and
    /// then reads the same `class = ...` literal through
    /// [`Self::structure_class_literal`]. `class = NULL` clears the
    /// attribute instead of attaching one, and a literal the stage would
    /// report as `ClassLiteral::Unknown` (a dynamic class) leaves the
    /// class open-world, so neither establishes dispatch.
    fn structure_args_attach_known_class(&self, args: &[Arg], scope: &Scope) -> bool {
        let matched = match_argument_names(
            &[".Data", "..."],
            args.iter()
                .map(|arg| arg.name.as_deref().map(semantic_argument_name)),
        );
        let Some(payload) = matched.arg_for_param(0) else {
            return false;
        };
        let exact_payloads = args
            .iter()
            .filter(|arg| arg.name.as_deref().map(semantic_argument_name) == Some(".Data"))
            .count();
        let partial_payloads = args
            .iter()
            .filter(|arg| {
                arg.name
                    .as_deref()
                    .map(semantic_argument_name)
                    .is_some_and(|name| !name.is_empty() && ".Data".starts_with(name))
            })
            .count();
        if exact_payloads > 1
            || (exact_payloads == 0 && partial_payloads > 1)
            || args.iter().enumerate().any(|(index, arg)| {
                matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                    || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_))
                    || (index != payload && arg.name.is_none())
            })
        {
            return false;
        }
        // A literal NULL payload degrades the whole construction to
        // `unknown` in the constructor stage; other payloads cannot be
        // proven NULL read-only, and only a provable one matters here.
        if matches!(&args[payload].value, Expr::Null(_)) {
            return false;
        }
        let mut class_args = args.iter().filter(|arg| {
            matches!(
                arg.name.as_deref().map(semantic_argument_name),
                Some("class")
            )
        });
        let class_arg = match (class_args.next(), class_args.next()) {
            // Absent, or repeated (a runtime-invalid binding the stage
            // refuses to arbitrate): no class is established either way.
            (Some(single), None) => single,
            _ => return false,
        };
        if matches!(&class_arg.value, Expr::Null(_)) {
            return false;
        }
        matches!(
            self.structure_class_literal(&class_arg.value, scope),
            ClassLiteral::Single(_) | ClassLiteral::Multi(_)
        )
    }

    /// Whether a proven `methods::new(...)` call attaches a *known*
    /// class, mirroring the `Class`-binding guards of
    /// [`Self::infer_methods_new`]: an unambiguous `Class` actual (the
    /// stage's duplicate-exact/partial-partial tolerance included), no
    /// `...`/parser-opaque arguments, and a non-empty string literal
    /// outside [`crate::semantic_lists::PLAIN_NEW_CLASSES`] (those lower
    /// to the payload's own plain shape).
    fn methods_new_args_attach_known_class(&self, args: &[Arg]) -> bool {
        let matched = match_argument_names(
            &["Class", "..."],
            args.iter()
                .map(|arg| arg.name.as_deref().map(semantic_argument_name)),
        );
        let Some(class_index) = matched.arg_for_param(0) else {
            return false;
        };
        let exact = args
            .iter()
            .filter(|arg| arg.name.as_deref().map(semantic_argument_name) == Some("Class"))
            .count();
        let partial = args
            .iter()
            .filter(|arg| {
                arg.name
                    .as_deref()
                    .map(semantic_argument_name)
                    .is_some_and(|name| !name.is_empty() && "Class".starts_with(name))
            })
            .count();
        if exact > 1
            || (exact == 0 && partial > 1)
            || args.iter().any(|arg| {
                matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                    || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_))
            })
        {
            return false;
        }
        matches!(
            &args[class_index].value,
            Expr::String(class, _)
                if !class.is_empty()
                    && !crate::semantic_lists::PLAIN_NEW_CLASSES.contains(&class.as_str())
        )
    }

    /// The atomic-constructor stage of `infer_call`: `c`, `list`,
    /// `data.frame`, `t`, and `as.data.frame`.
    pub(crate) fn infer_atomic_constructor_call(
        &mut self,
        lookup_name: &str,
        args: &[Arg],
        arg_types: &[RType],
    ) -> Option<RType> {
        // Built-in: `c(...)` concatenates and produces the common mode.
        if lookup_name == "c" {
            // `recursive` and `use.names` are controls matched after
            // `...`, never elements: R excludes them from the result's
            // mode, length, and names. A `recursive` that may be TRUE
            // can flatten list elements, a length no exact or bounded
            // claim here survives.
            let is_control =
                |arg: &Arg| matches!(arg.name.as_deref(), Some("recursive" | "use.names"));
            if args.iter().any(is_control) {
                let may_flatten = args.iter().any(|arg| {
                    arg.name.as_deref() == Some("recursive")
                        && !matches!(arg.value, Expr::Logical(false, _))
                });
                let (elem_args, elem_types): (Vec<Arg>, Vec<RType>) = args
                    .iter()
                    .zip(arg_types)
                    .filter(|(arg, _)| !is_control(arg))
                    .map(|(arg, ty)| (arg.clone(), ty.clone()))
                    .unzip();
                // Any element not proven atomic may be a list that the
                // flattening empties or turns atomic
                // (`c(list(1, NULL), recursive = TRUE)` is the double 1).
                if may_flatten
                    && elem_types
                        .iter()
                        .any(|ty| !(ty.mode.is_atomic() || ty.mode == Mode::Null))
                {
                    return Some(RType::unknown());
                }
                let result = self.infer_c(&elem_args, &elem_types);
                if let Some(schema) = build_named_schema(&elem_types, &elem_args)
                    .filter(|_| elem_args.iter().any(|argument| argument.name.is_some()))
                {
                    return Some(result.with_columns(Arc::new(schema)));
                }
                return Some(result);
            }
            let result = self.infer_c(args, arg_types);
            if let Some(schema) = build_named_schema(arg_types, args)
                .filter(|_| args.iter().any(|argument| argument.name.is_some()))
            {
                return Some(result.with_columns(Arc::new(schema)));
            }
            return Some(result);
        }
        if lookup_name == "list" {
            return Some(self.infer_list(arg_types, args));
        }
        // `data.frame(...)`: a record constructor. Same column-schema
        // logic as `list(...)`, but the result is classed
        // "data.frame" and column lengths are coerced to a common
        // length (R recycles; for v1 we take the max of the known
        // lengths).
        if lookup_name == "data.frame" {
            if args.len() == 1
                && args[0].name.is_none()
                && let Some(schema) = arg_types[0].columns.clone()
            {
                return Some(
                    RType::new(Mode::List, Length::Known(schema.columns.len()))
                        .with_class(ClassVector::single("data.frame"))
                        .with_columns(schema),
                );
            }
            return Some(self.infer_data_frame(arg_types, args));
        }

        if lookup_name == "t" {
            return Some(arg_types.first().cloned().unwrap_or_else(RType::unknown));
        }

        if lookup_name == "as.data.frame"
            && let Some(input) = arg_types.first()
            && let Some(schema) = input.columns.clone()
            && !schema.is_empty()
        {
            return Some(
                RType::new(Mode::List, Length::Known(schema.columns.len()))
                    .with_class(ClassVector::single("data.frame"))
                    .with_columns(schema),
            );
        }
        None
    }

    /// The literal-length constructor stage of `infer_call`: `vector`,
    /// `rep`, `seq`, and `seq.int` pin their result length from literal
    /// arguments; the typeshed entries for these names conservatively
    /// return `Length::Unknown`.
    pub(crate) fn infer_literal_length_call(
        &self,
        lookup_name: &str,
        args: &[Arg],
        arg_types: &[RType],
    ) -> Option<RType> {
        if lookup_name == "vector" {
            return Some(self.infer_vector(args));
        }
        if lookup_name == "rep" {
            return Some(self.infer_rep(args, arg_types));
        }
        if lookup_name == "seq" || lookup_name == "seq.int" {
            return Some(self.infer_seq(lookup_name, args, arg_types));
        }
        None
    }

    pub(crate) fn infer_c(&mut self, args: &[Arg], arg_types: &[RType]) -> RType {
        if arg_types.is_empty() {
            return RType::new(Mode::Null, Length::Zero);
        }
        let mut mode = Mode::Null;
        let mut total_len = Some(0usize);
        let mut min_total = 0usize;
        for t in arg_types {
            mode = mode.combine_result(if t.mode == Mode::Union {
                Mode::Opaque
            } else {
                t.mode
            });
            // The exact total may be gone, but the sum of proven lower
            // bounds is a sound lower bound for the concatenation.
            min_total = min_total.saturating_add(t.length.min_possible());
            match t.length {
                Length::Zero => {}
                Length::One => total_len = total_len.and_then(|total| total.checked_add(1)),
                Length::Known(n) => total_len = total_len.and_then(|total| total.checked_add(n)),
                Length::Nonempty | Length::AtLeast(_) | Length::Unknown => total_len = None,
            }
        }
        let length = if args
            .iter()
            .any(|a| matches!(a.value, Expr::Unknown(_) | Expr::Missing(_)))
        {
            Length::Unknown
        } else {
            match total_len {
                Some(total) => Length::Known(total),
                None => Length::lower_bound(min_total),
            }
        };
        let mut result = RType::new(mode, length);
        // A fully observed unclassed concatenation keeps its value proof.
        if matches!(mode, Mode::Integer | Mode::Double)
            && arg_types.iter().all(|t| t.value_facts.all_values_known)
            && arg_types
                .iter()
                .all(|t| !t.class.is_unknown() && !t.class.has_known_class())
        {
            result.value_facts = arg_types
                .iter()
                .map(|t| t.value_facts)
                .reduce(ry_core::types::ValueFacts::merge_vector)
                .unwrap_or_default();
        }
        if mode.is_unknown() {
            result.with_class(ClassVector::unknown())
        } else {
            result
        }
    }

    /// Infer the type of `list(...)`: a list whose length equals the
    /// argument count, plus a column schema from named args (positional
    /// args get R's auto-generated `[[i]]` names). The schema is built
    /// even when only some args are named, mirroring R's
    /// `list(a = 1, "x")` producing names `c("a", "2")`; it is what
    /// powers `df$col` / `df[["col"]]` resolution downstream.
    pub(crate) fn infer_list(&mut self, arg_types: &[RType], args: &[Arg]) -> RType {
        // `...` expands to however many arguments the caller supplied,
        // including none, so only the other arguments are counted (#864).
        let is_dots = |arg: &Arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "...");
        let fixed = args.iter().filter(|arg| !is_dots(arg)).count();
        let length = if args.iter().any(is_dots) {
            Length::lower_bound(fixed)
        } else {
            Length::Known(arg_types.len())
        };
        let base = RType::new(Mode::List, length);
        let mut schema = build_named_schema(arg_types, args).unwrap_or(ColumnSchema {
            columns: Vec::new(),
            complete: true,
            locally_constructed: true,
        });
        schema.locally_constructed = true;
        // `...` (and parser-opaque splice forms) can contribute arbitrary
        // fields at runtime. Preserve fields we can see, but never treat the
        // result as a closed record: absent fields are not known NULL and
        // cannot justify missing-column diagnostics.
        if args
            .iter()
            .any(|arg| is_dots(arg) || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_)))
        {
            schema.complete = false;
        }
        base.with_columns(Arc::new(schema))
    }

    /// Infer the type of `data.frame(...)`: the same column-schema logic
    /// as `list(...)`, but the result is classed `"data.frame"` and
    /// column lengths are coerced to a common length (R recycles; v1
    /// takes the max of the known lengths and propagates it onto each
    /// column so `df$col` returns a vector of the right length). Known
    /// metadata arguments (`row.names`, `check.names`, ...) are not
    /// columns and are dropped from the schema.
    pub(crate) fn infer_data_frame(&mut self, arg_types: &[RType], args: &[Arg]) -> RType {
        use crate::semantic_lists::METADATA_ARGS;
        let mut filtered_types: Vec<RType> = Vec::with_capacity(arg_types.len());
        let mut filtered_args: Vec<Arg> = Vec::with_capacity(args.len());
        for (i, a) in args.iter().enumerate() {
            if let Some(n) = a.name.as_deref() {
                if METADATA_ARGS.contains(&n) {
                    continue;
                }
            }
            filtered_types.push(arg_types[i].clone());
            filtered_args.push(a.clone());
        }

        let common_len = longest_arg_length(&filtered_types);

        let coerced_types: Vec<RType> = filtered_types
            .iter()
            .map(|t| RType {
                mode: t.mode,
                length: common_len,
                class: t.class.clone(),
                // Nested column schemas on a data-frame column would
                // mean nested data frames; v1 keeps those opaque.
                columns: None,
                // fn_sig is meaningless on a data-frame column.
                fn_sig: None,
                members: None,
                value_facts: t.value_facts,
            })
            .collect();

        // Reuse the named-schema builder, then patch the coerced types
        // in (the builder uses the original arg_types verbatim).
        let schema = build_data_frame_schema(&coerced_types, &filtered_args);

        let class = ClassVector::single("data.frame");
        let base = RType::new(Mode::List, Length::Known(filtered_types.len())).with_class(class);
        match schema {
            Some(s) => base.with_columns(Arc::new(s)),
            None => base,
        }
    }

    // Infer the result type of `vector(mode, length)`: pin the mode and
    // length from literal arguments when possible.
    pub(crate) fn infer_vector(&self, args: &[Arg]) -> RType {
        let bindings = match_arguments(&["mode", "length"], args);
        let mode_expr = bindings.arg_for_param(0).map(|index| &args[index].value);
        let mode = match mode_expr {
            Some(Expr::String(mode, _)) => match mode.as_str() {
                "logical" => Mode::Logical,
                "integer" => Mode::Integer,
                "numeric" | "double" => Mode::Double,
                "complex" => Mode::Complex,
                "character" => Mode::Character,
                "raw" => Mode::Raw,
                "list" | "expression" => Mode::List,
                _ => Mode::Opaque,
            },
            None => Mode::Logical,
            _ => Mode::Opaque,
        };

        let length =
            size_argument_length(bindings.arg_for_param(1).map(|index| &args[index].value), 0);

        RType::new(mode, length)
    }

    /// Infer `rep(x, times, length.out, each)`: length is
    /// `length(x) * times * each` with unsupplied counts defaulting to 1,
    /// keeping `x`'s mode, class, and schema. A literal `length.out` wins
    /// over both counts, as in R. Arguments bind with R's exact, partial,
    /// then positional matching (`rep(x, 2, 3)` is `length.out = 3`,
    /// `rep(x, len = 3)` too). Counts are read from the raw AST, not the
    /// inferred `RType`, because the type lattice discards the runtime
    /// value (a supplied non-literal means `Length::Unknown`).
    pub(crate) fn infer_rep(&self, args: &[Arg], arg_types: &[RType]) -> RType {
        let matched = match_arguments(&["x", "times", "length.out", "each"], args);
        let x_type = matched
            .arg_for_param(0)
            .and_then(|i| arg_types.get(i).cloned())
            .unwrap_or(RType::unknown());
        if !matched.is_one_to_one() {
            return RType {
                length: Length::Unknown,
                ..x_type
            };
        }
        let count_arg = |param| {
            matched
                .arg_for_param(param)
                .map(|i| extract_literal_int(&args[i].value))
        };
        // R ignores `length.out = NA`; any other supplied `length.out`
        // fixes the length on its own. A zero-length atomic `x` pads with
        // `NA`, but `NULL` stays `NULL`, so only a proven non-NULL vector
        // takes the requested length.
        let length_out = matched
            .arg_for_param(2)
            .filter(|&i| !matches!(args[i].value, Expr::Na(..)));
        if let Some(i) = length_out {
            let length = match extract_literal_int(&args[i].value) {
                Some(n) if n >= 0 && x_type.mode == Mode::Null => x_type.length,
                Some(n) if n >= 0 && (x_type.mode.is_atomic() || x_type.mode == Mode::List) => {
                    match n {
                        0 => Length::Zero,
                        1 => Length::One,
                        n => Length::Known(n as usize),
                    }
                }
                _ => Length::Unknown,
            };
            return RType { length, ..x_type };
        }
        // Track `times` / `each` as `Option<Option<i64>>`:
        //   * outer None      -> not supplied (use default 1)
        //   * outer Some(None) -> supplied but non-literal (Unknown)
        //   * outer Some(Some(n)) -> supplied literal value n
        let times = count_arg(1);
        let each = count_arg(3);
        // Resolve `times` and `each` through the shared count resolver.
        // Unsupplied -> 1; a non-literal or negative literal -> the length is
        // unknown (R errors or recycles in ways we can't model, so we stay
        // conservative rather than pin a wrong length).
        let Some(times_n) = rep_count(times) else {
            return RType {
                length: Length::Unknown,
                ..x_type
            };
        };
        let Some(each_n) = rep_count(each) else {
            return RType {
                length: Length::Unknown,
                ..x_type
            };
        };
        // Compute the total length, normalizing so we never emit
        // `Length::Known(0)` (which violates the `Known(n > 1)`
        // invariant) or `Length::Known(1)` (use `Length::One` instead).
        // A zero total (e.g. `rep(x, times = 0)`) becomes `Length::Zero`.
        let length = match x_type.length {
            Length::Zero => Length::Zero,
            Length::One => {
                let total = times_n.saturating_mul(each_n);
                match total {
                    0 => Length::Zero,
                    1 => Length::One,
                    n => Length::Known(n),
                }
            }
            Length::Known(xn) => {
                let total = xn.saturating_mul(times_n).saturating_mul(each_n);
                match total {
                    0 => Length::Zero,
                    1 => Length::One,
                    n => Length::Known(n),
                }
            }
            // Repetition scales a proven lower bound unless a count of
            // zero empties the result (unknown counts returned early).
            Length::AtLeast(xn) => {
                let total = xn.saturating_mul(times_n).saturating_mul(each_n);
                if total == 0 {
                    Length::Zero
                } else {
                    Length::lower_bound(total)
                }
            }
            Length::Nonempty => {
                if times_n > 0 && each_n > 0 {
                    Length::Nonempty
                } else {
                    Length::Zero
                }
            }
            Length::Unknown => Length::Unknown,
        };
        RType { length, ..x_type }
    }

    /// Infer the result type of `seq(from, to, by)` / `seq.int(...)`.
    /// Literal forms pin the length exactly: `|to - from| / |by| + 1`
    /// (R rounds to the nearest whole step in range), `length.out = n`
    /// when supplied (it wins over `by`, as R documents), or
    /// `|to - from| + 1` when `by` is absent (R defaults it to +/-1).
    /// Otherwise the length is `Length::Unknown`. The mode depends on
    /// values (see [`seq_call_mode`]); one the checker cannot prove is
    /// `integer | double`.
    pub(crate) fn infer_seq(&self, lookup_name: &str, args: &[Arg], arg_types: &[RType]) -> RType {
        // `seq.default`'s formals; `seq.int` takes the same ones. The
        // `SEQ_*` indices below point into this list.
        let bindings = match_arguments(
            &["from", "to", "by", "length.out", "along.with", "..."],
            args,
        );
        // An actual that `...` absorbs (a forwarded `...`, an extra
        // positional) can stand for any formal.
        if args.len() != arg_types.len() || bindings.param_for_arg.iter().any(Option::is_none) {
            return integer_or_double_type(Length::Unknown);
        }
        let literal = |formal: usize| {
            bindings
                .arg_for_param(formal)
                .map(|i| extract_literal_int(&args[i].value))
        };
        let from_val = literal(SEQ_FROM).flatten();
        let to_val = literal(SEQ_TO).flatten();
        let by_literal = literal(SEQ_BY);
        let lo_literal = literal(SEQ_LENGTH_OUT);
        let along_supplied = bindings.bound_params[SEQ_ALONG_WITH];

        // A supplied `length.out`, `by` or `along.with` that isn't a
        // literal leaves the length unknown. (`from`/`to` are handled
        // below: `extract_literal_int` returns None for them, which makes
        // the formula fall through to Unknown.)
        let length = if along_supplied
            || matches!(lo_literal, Some(None))
            || matches!(by_literal, Some(None))
        {
            Length::Unknown
        } else if let Some(Some(n)) = lo_literal {
            // `length.out` wins over `by` when both are present.
            if n >= 0 {
                Length::Known(n as usize)
            } else {
                Length::Unknown
            }
        } else if let (Some(f), Some(t)) = (from_val, to_val) {
            match by_literal.flatten() {
                // by == 0: R errors at runtime; model as Unknown.
                Some(0) => Length::Unknown,
                Some(b) => {
                    let diff = (t - f).unsigned_abs() as usize;
                    let step = b.unsigned_abs() as usize;
                    Length::Known(diff / step + 1)
                }
                // by not supplied: R defaults to +/-1.
                None => Length::Known((t - f).unsigned_abs() as usize + 1),
            }
        } else {
            Length::Unknown
        };
        let supplied = std::array::from_fn(|formal| {
            bindings
                .arg_for_param(formal)
                .map(|i| (&args[i].value, &arg_types[i]))
        });
        match seq_call_mode(lookup_name == "seq.int", supplied, lo_literal.flatten()) {
            NumericMode::Integer => RType::new(Mode::Integer, length),
            NumericMode::Double => RType::new(Mode::Double, length),
            NumericMode::IntegerOrDouble => integer_or_double_type(length),
            NumericMode::Unknown => RType::unknown(),
        }
    }

    pub(crate) fn apply_sig(
        &mut self,
        sig: &FunctionSig,
        arg_types: &[RType],
        args: &[Arg],
    ) -> RType {
        // Match named arguments to parameters so that `arg0` refers to
        // the first *parameter* (by name), not the first positional arg.
        // When `sig.params` is empty or only contains `...`, fall back
        // to raw positional indexing.
        let matched = if sig.params.is_empty()
            || sig.params.iter().all(|p| p.name == "...")
            // When the caller has argument *types* but no `Arg` slice
            // (e.g. `callback_return_type` inferring a typeshed callback
            // from the element types a higher-order function will pass),
            // named-arg matching has nothing to work from: use the types
            // positionally so `arg0`/`arg1`/... resolve correctly.
            || args.is_empty()
        {
            arg_types.to_vec()
        } else {
            match_args_to_params(&sig.params, args, arg_types)
        };
        let first = matched.first().cloned().unwrap_or(RType::unknown());
        match &sig.return_ {
            ReturnSpec::Slot(slot) => {
                let result = match slot {
                    ReturnSlot::Arg0 => first,
                    ReturnSlot::ConcatOfArgs => self.infer_c(args, arg_types),
                };
                match semantic_return_length(
                    sig.return_length.as_ref(),
                    &sig.params,
                    args,
                    arg_types,
                ) {
                    Some(length) => with_selected_elements(result, length),
                    None => result,
                }
            }
            ReturnSpec::Concrete(c) => {
                let mut integer_or_double = false;
                let mut union_return = false;
                let mode = if let Some(mode) = concrete_json_mode(&c.mode) {
                    mode
                } else {
                    match JsonMode::parse(&c.mode) {
                        // A union is built below, once the length is known.
                        Some(JsonMode::Union) => {
                            union_return = true;
                            Mode::Opaque
                        }
                        // "double_or_int" follows the numeric data arguments.
                        // An `integer | double` result is built below, once
                        // the length is known.
                        Some(JsonMode::DoubleOrInt) => {
                            match double_or_int_mode(&double_or_int_inputs(
                                sig, args, arg_types, &first,
                            )) {
                                NumericMode::Integer => Mode::Integer,
                                NumericMode::Double => Mode::Double,
                                NumericMode::IntegerOrDouble => {
                                    integer_or_double = true;
                                    Mode::Double
                                }
                                NumericMode::Unknown => Mode::Opaque,
                            }
                        }
                        // "arg0" as a mode spec: use the first param's mode.
                        Some(JsonMode::Arg0) => first.mode,
                        // "arg2" as a mode spec: use the third param's mode.
                        Some(JsonMode::Arg2) => {
                            matched.get(2).map(|t| t.mode).unwrap_or(Mode::Opaque)
                        }
                        // "yes_or_no": join of the second and third params'
                        // modes (for `ifelse(test, yes, no)`). The join may be
                        // a union; taking `.mode` drops the members and would
                        // build a malformed union below, so collapse a union
                        // mode to opaque.
                        Some(JsonMode::YesOrNo) => {
                            let yes = matched.get(1).cloned().unwrap_or(RType::unknown());
                            let no = matched.get(2).cloned().unwrap_or(RType::unknown());
                            let joined = yes.join(no).mode;
                            if matches!(joined, Mode::Union) {
                                Mode::Opaque
                            } else {
                                joined
                            }
                        }
                        _ => Mode::Opaque,
                    }
                };
                // The arg-N mode specs copy a param's mode verbatim; if a
                // caller passes a union there, that mode is `Mode::Union`
                // and would build a malformed union. Collapse to opaque. An
                // environment or language mode would lose its class here.
                let mode = if matches!(mode, Mode::Union) || mode.is_known_non_vector() {
                    Mode::Opaque
                } else {
                    mode
                };
                let length = match JsonLength::parse(&c.length) {
                    Some(JsonLength::Arg0) => first.length,
                    Some(JsonLength::Arg1) => {
                        matched.get(1).map(|t| t.length).unwrap_or(Length::Unknown)
                    }
                    Some(JsonLength::Arg2) => {
                        matched.get(2).map(|t| t.length).unwrap_or(Length::Unknown)
                    }
                    // Longest of all args' lengths (for paste/paste0/sprintf).
                    Some(JsonLength::LongestArg) => longest_arg_length(arg_types),
                    // `file.path` family: ?file.path returns a path for
                    // every element only when all arguments have positive
                    // length — any zero-length argument makes the result
                    // empty (unlike paste, which recycles "" for zeros), so
                    // an argument that may be empty leaves no lower bound.
                    Some(JsonLength::LongestArgOrZero) => {
                        if arg_types.iter().any(|ty| matches!(ty.length, Length::Zero)) {
                            Length::Zero
                        } else if arg_types.iter().any(|ty| ty.length.min_possible() == 0) {
                            Length::Unknown
                        } else {
                            longest_arg_length(arg_types)
                        }
                    }
                    // Number of arguments (for list()).
                    Some(JsonLength::NArgs) => Length::Known(args.len()),
                    Some(JsonLength::Test) => first.length,
                    // Literal lengths and a missing spec alike.
                    literal => json_length_to_length(literal),
                };
                let length = semantic_return_length(
                    sig.return_length.as_ref(),
                    &sig.params,
                    args,
                    arg_types,
                )
                .unwrap_or(length);
                if integer_or_double {
                    return integer_or_double_type(length);
                }
                if union_return {
                    return json_rtype_with_length(c, length);
                }
                let mut result = RType::new(mode, length);
                if c.class.is_empty() && mode.is_unknown() {
                    // An opaque return without a class entry knows nothing
                    // about the class; absent metadata must not read as a
                    // proven-empty class vector (see `json_rtype_scalar`).
                    result.class = ClassVector::unknown();
                }
                if !c.class.is_empty() {
                    let refs: Vec<&str> = c.class.iter().map(String::as_str).collect();
                    result = result.with_class(ClassVector::from_slice(&refs));
                }
                if !c.columns.is_empty() {
                    let cols: Vec<(String, RType)> = c
                        .columns
                        .iter()
                        .map(|(name, child)| (name.clone(), json_rtype_scalar(child)))
                        .collect();
                    result = result.with_columns(Arc::new(ColumnSchema {
                        columns: cols,
                        complete: true,
                        locally_constructed: false,
                    }));
                }
                result
            }
        }
    }
}

/// A slot result whose elements a `return_length` rule re-counts (`head`,
/// `unique`, set operations): mode and class carry over, but facts about the
/// whole value no longer hold, except that a nonempty selection from a fully
/// known, NA-free constant vector still holds exactly that value. A data
/// frame keeps its columns, and so its length, while the selection changes
/// its row count; any other record loses its shape, since its elements are
/// what was selected.
pub(crate) fn with_selected_elements(mut value: RType, length: Length) -> RType {
    let facts = value.value_facts;
    let constant = facts.all_values_known
        && !facts.prior_na
        && facts.new_na == ry_core::types::NewNaProvenance::None
        && facts.numeric_bounds.is_some_and(|(low, high)| low == high);
    if !(constant && matches!(length, Length::One | Length::Nonempty)) {
        value.value_facts = facts.for_subset();
    }
    match value.columns.take() {
        Some(schema) if value.class.contains("data.frame") => {
            let mut schema = (*schema).clone();
            for (_, column) in &mut schema.columns {
                column.length = Length::Unknown;
                column.value_facts = column.value_facts.for_subset();
            }
            value.columns = Some(Arc::new(schema));
        }
        _ => value.length = length,
    }
    value
}

/// Resolve a `rep` repetition count.
///
/// `None` (argument not supplied) is R's default of 1. A supplied non-literal
/// or negative literal has no count we can pin, so it yields `None` and the
/// caller reports an unknown length.
fn rep_count(value: Option<Option<i64>>) -> Option<usize> {
    match value {
        None => Some(1),
        Some(Some(n)) if n >= 0 => Some(n as usize),
        Some(_) => None,
    }
}

/// Indices of `seq.default`'s formals, as matched in [`Checker::infer_seq`].
const SEQ_FROM: usize = 0;
const SEQ_TO: usize = 1;
const SEQ_BY: usize = 2;
const SEQ_LENGTH_OUT: usize = 3;
const SEQ_ALONG_WITH: usize = 4;

/// The storage mode of a `seq()` / `seq.int()` call, following
/// `seq.default`:
///
/// - `seq(n)` is `1L:n` for a numeric scalar `n`, and `seq(from, to)` is
///   `from:to`, so both reuse the value-sensitive rule of `:`
///   ([`RType::seq`]): `seq(1, 10)` is integer, `seq(1.5, 10)` double.
/// - With `by`, the result keeps integer storage only when `from`, `to`
///   and `by` are all integer or logical (`seq(1, 10, by = 2)` is double).
///   `seq.int` also keeps it for whole doubles (`seq.int(1, 10, by = 1)`
///   is integer), so only `seq` proves double from double endpoints.
/// - `seq(length.out = n)` and `seq(along.with = x)` alone are
///   `seq_len()`. `seq` with a positive literal `length.out` and double
///   endpoints is double; `seq.int(1, 10, length.out = 4)` is integer.
///
/// Anything else depends on values the checker does not track.
fn seq_call_mode(
    seq_int: bool,
    supplied: [Option<(&Expr, &RType)>; SEQ_ALONG_WITH + 1],
    length_out: Option<i64>,
) -> NumericMode {
    let [from, to, by, length_out_arg, along_with] = supplied;
    let length_out_supplied = length_out_arg.is_some();
    let along_with = along_with.is_some();
    // `is.logint()` in `seq.default`: integer or logical storage that is
    // not an object.
    let logint = |(_, ty): (&Expr, &RType)| {
        matches!(ty.mode, Mode::Integer | Mode::Logical) && ty.class.is_classless()
    };
    let plain_double =
        |(_, ty): (&Expr, &RType)| ty.mode == Mode::Double && ty.class.is_classless();
    // `f1:f2` with two factor endpoints is their interaction, and inferred
    // classes are not proof that a value is no factor; a literal is.
    let literal = |(expr, _): (&Expr, &RType)| {
        matches!(
            expr,
            Expr::Integer(..) | Expr::Double(..) | Expr::Logical(..)
        )
    };
    let colon = |from: RType, to: RType, non_factor: bool| match from.seq(to, non_factor).mode {
        Mode::Integer => NumericMode::Integer,
        Mode::Double => NumericMode::Double,
        Mode::Union => NumericMode::IntegerOrDouble,
        _ => NumericMode::Unknown,
    };
    let one = RType::scalar(Mode::Integer);
    match (from, to, by, length_out_supplied, along_with) {
        (None, None, None, true, false) | (None, None, None, false, true) => NumericMode::Integer,
        (Some(from), None, None, false, false) => colon(one, from.1.clone(), true),
        (None, Some(to), None, false, false) => colon(one, to.1.clone(), true),
        (Some(from), Some(to), None, false, false) => {
            colon(from.1.clone(), to.1.clone(), literal(from) || literal(to))
        }
        (Some(from), Some(to), Some(by), false, false) => {
            if logint(from) && logint(to) && logint(by) {
                NumericMode::Integer
            } else if !seq_int && plain_double(from) && plain_double(to) {
                NumericMode::Double
            } else {
                NumericMode::IntegerOrDouble
            }
        }
        (from, to, None, true, false)
            if !seq_int
                && length_out.is_some_and(|n| n > 0)
                && (from.is_some() || to.is_some())
                && from.is_none_or(plain_double)
                && to.is_none_or(plain_double) =>
        {
            NumericMode::Double
        }
        _ => NumericMode::IntegerOrDouble,
    }
}

/// The storage mode a numeric result can take when R picks it from values
/// the checker may not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumericMode {
    Integer,
    Double,
    IntegerOrDouble,
    Unknown,
}

/// `integer | double` at `length`, for a result whose storage mode depends
/// on values the checker does not know.
fn integer_or_double_type(length: Length) -> RType {
    RType::union(Arc::from(vec![
        RType::new(Mode::Integer, length),
        RType::new(Mode::Double, length),
    ]))
}

/// The data arguments a `double_or_int` return follows: everything `...`
/// absorbs when the signature leads with it (`sum(..., na.rm)`, `max(...)`),
/// else the first formal. `na.rm` is a formal of every dots-led member in
/// R even where the stub omits it, so it never counts as data.
fn double_or_int_inputs(
    sig: &FunctionSig,
    args: &[Arg],
    arg_types: &[RType],
    first: &RType,
) -> Vec<RType> {
    if sig.params.first().is_none_or(|param| param.name != "...") {
        return vec![first.clone()];
    }
    if args.is_empty() {
        return arg_types.to_vec();
    }
    // `...` never binds a formal of its own: what it absorbs is unbound.
    let bindings = match_params(&sig.params, args);
    args.iter()
        .zip(arg_types)
        .zip(&bindings.param_for_arg)
        .filter(|((arg, _), formal)| formal.is_none() && arg.name.as_deref() != Some("na.rm"))
        .map(|((_, ty), _)| ty.clone())
        .collect()
}

/// Resolve a `double_or_int` return from its data arguments. R keeps
/// integer storage when every input is logical or integer
/// (`sum(c(TRUE, FALSE))`, `abs(TRUE)` and `max(TRUE)` are integer) and
/// widens to double once any input is double. Character and complex input
/// keeps neither mode (`max("a")` is character, `sum(1i)` is complex), and
/// the members disagree on it (`abs(1i)` is double), so an input of any
/// other known mode leaves the result mode unknown.
fn double_or_int_mode(inputs: &[RType]) -> NumericMode {
    let mut all_integer = true;
    let mut any_double = false;
    for input in inputs {
        let members: &[RType] = match (&input.mode, &input.members) {
            (Mode::Union, Some(members)) => members,
            _ => std::slice::from_ref(input),
        };
        let mut can_be_integer = false;
        let mut can_be_double = false;
        for member in members {
            match member.mode {
                Mode::Logical | Mode::Integer => can_be_integer = true,
                // An unproven input keeps the historical double
                // approximation: it still proves the result atomic, which
                // `mean(x)$value` (RY061) relies on.
                Mode::Double => can_be_double = true,
                mode if mode.is_unknown() => can_be_double = true,
                _ => return NumericMode::Unknown,
            }
        }
        any_double |= !can_be_integer;
        all_integer &= !can_be_double;
    }
    if inputs.is_empty() || any_double {
        NumericMode::Double
    } else if all_integer {
        NumericMode::Integer
    } else {
        NumericMode::IntegerOrDouble
    }
}

pub(crate) fn semantic_return_length(
    semantics: Option<&ReturnLengthSpec>,
    signature_params: &[ParamSpec],
    args: &[Arg],
    arg_types: &[RType],
) -> Option<Length> {
    let semantics = semantics?;
    // Callback inference supplies argument types without source arguments.
    // Formal semantic binding is unavailable there, so retain the declared
    // return-length fallback instead of treating the callback as argumentless.
    if args.is_empty() && !arg_types.is_empty() {
        return None;
    }
    let bindings = match_params(signature_params, args);
    let bound_args = |param: &str| {
        signature_params
            .iter()
            .position(|candidate| candidate.name == param)
            .into_iter()
            .flat_map(|parameter_index| {
                bindings.param_for_arg.iter().enumerate().filter_map(
                    move |(argument_index, bound)| {
                        (*bound == Some(parameter_index)).then_some(argument_index)
                    },
                )
            })
    };
    match semantics {
        ReturnLengthSpec::ParamValue {
            param,
            default_length,
        } => Some(size_argument_length(
            bound_args(param).next().map(|index| &args[index].value),
            *default_length,
        )),
        ReturnLengthSpec::ZeroIfAnyParamZero { params } => {
            if params
                .iter()
                .flat_map(|param| bound_args(param))
                .filter_map(|index| arg_types.get(index))
                .any(|ty| matches!(ty.length, Length::Zero))
            {
                Some(Length::Zero)
            } else {
                Some(Length::Unknown)
            }
        }
        ReturnLengthSpec::SameEmptiness { param } => Some(
            match bound_args(param)
                .next()
                .and_then(|index| arg_types.get(index))
                .map(|ty| ty.length)
            {
                Some(Length::Zero | Length::Known(0)) => Length::Zero,
                Some(Length::One | Length::Known(_) | Length::Nonempty | Length::AtLeast(_)) => {
                    Length::Nonempty
                }
                _ => Length::Unknown,
            },
        ),
        // An unbound parameter proves nothing about its default's length.
        ReturnLengthSpec::ZeroIfAllParamsZero { params } => {
            if params.iter().all(|param| {
                let mut bound = bound_args(param).peekable();
                bound.peek().is_some()
                    && bound.all(|index| {
                        arg_types
                            .get(index)
                            .is_some_and(|ty| matches!(ty.length, Length::Zero))
                    })
            }) {
                Some(Length::Zero)
            } else {
                Some(Length::Unknown)
            }
        }
        ReturnLengthSpec::RecycledValues(spec) => {
            let value_types: Vec<_> = args
                .iter()
                .zip(arg_types)
                .enumerate()
                .filter(|(index, _)| {
                    let bound = bindings.param_for_arg[*index]
                        .and_then(|parameter| signature_params.get(parameter))
                        .map(|parameter| parameter.name.as_str());
                    bound.is_some_and(|name| spec.value_params.iter().any(|value| value == name))
                        // Unmatched arguments after `...` are captured by
                        // it, while exact controls after `...` were bound in
                        // the first matching pass and are excluded above.
                        || (bound.is_none()
                            && bindings.dots.is_some()
                            && spec.value_params.iter().any(|value| value == "..."))
                })
                .map(|(_, (_, ty))| ty.clone())
                .collect();
            // Controls and values share the same formal binding result.
            // `control_params` is semantic: only a declared control may
            // influence a recycled-values rule.
            let bound_control = |param: &str| {
                spec.control_params
                    .iter()
                    .any(|control| control == param)
                    .then(|| bound_args(param).next())
                    .flatten()
            };
            if let Some(index) = bound_control(&spec.collapse.param) {
                // `collapse = NULL` leaves the recycled vector intact. An
                // unknown control is not evidence of a scalar result.
                if matches!(arg_types[index].mode, Mode::Null) {
                    // Fall through to ordinary recycled-value length.
                } else if !arg_types[index].mode.is_unknown_or_non_vector()
                    && arg_types[index].mode != Mode::Union
                {
                    return Some(Length::One);
                } else {
                    return Some(Length::Unknown);
                }
            }
            if let Some(index) = bound_control(&spec.recycle0.param)
                && !matches!(args[index].value, Expr::Logical(false, _))
            {
                // A `recycle0` that may be TRUE empties the result on a
                // zero-length value, so a possibly-empty one leaves no bound.
                if matches!(args[index].value, Expr::Logical(true, _))
                    && value_types
                        .iter()
                        .any(|ty| matches!(ty.length, Length::Zero))
                {
                    return Some(Length::Zero);
                }
                if value_types.iter().any(|ty| ty.length.min_possible() == 0) {
                    return Some(Length::Unknown);
                }
            }
            if value_types.is_empty()
                || value_types
                    .iter()
                    .all(|ty| matches!(ty.length, Length::Zero))
            {
                Some(Length::Zero)
            } else {
                Some(longest_arg_length(&value_types))
            }
        }
    }
}

/// R truncates numeric sizes towards zero; dynamic and invalid sizes stay unknown.
fn size_argument_length(value: Option<&Expr>, default: usize) -> Length {
    let number = match value {
        None => return json_length_to_length(Some(JsonLength::Known(default))),
        Some(Expr::Integer(n, _)) => *n as f64,
        Some(Expr::Double(n, _)) => *n,
        Some(Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
            ..
        }) => match expr.as_ref() {
            Expr::Integer(n, _) => -(*n as f64),
            Expr::Double(n, _) => -*n,
            _ => return Length::Unknown,
        },
        _ => return Length::Unknown,
    };
    if !number.is_finite() || number.trunc() < 0.0 || number >= usize::MAX as f64 {
        return Length::Unknown;
    }
    json_length_to_length(Some(JsonLength::Known(number.trunc() as usize)))
}
