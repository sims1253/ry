use super::*;

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
        // Spelling aliases do not prove which function object was captured;
        // neither this model nor the base stub may lend them payload facts.
        if lookup_name == "structure"
            && (original_name != semantic_name
                || (matches!(original_callee, Expr::String(_, _)) && semantic_name.contains("::")))
        {
            return Some(RType::unknown());
        }
        if lookup_name == "structure" && self.structure_namespace_unavailable(semantic_name, scope)
        {
            return Some(RType::unknown());
        }
        if lookup_name == "structure"
            && !self.user_stubs.contains_key("base")
            && self.resolves_to_base_lenient(semantic_name, scope)
        {
            if self.resolves_to_base(semantic_name, scope)
                && !self.structure_bare_callee_unavailable(semantic_name, scope)
            {
                return Some(self.infer_structure_call(args, scope));
            }
            // An unproven binding or search path may supply another callable.
            // Do not borrow base-stub payload facts or force quoted arguments.
            return Some(RType::unknown());
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
                crate::resolve::SpecialCallProvenance::Proven => {}
                crate::resolve::SpecialCallProvenance::Ordinary => return None,
                crate::resolve::SpecialCallProvenance::Unknown
                | crate::resolve::SpecialCallProvenance::AmbientUncertainty => {
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
                RType::unknown().with_class(ClassVector::single(class))
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
            return Some(self.infer_seq(args, arg_types));
        }
        None
    }

    pub(crate) fn infer_c(&mut self, args: &[Arg], arg_types: &[RType]) -> RType {
        if arg_types.is_empty() {
            return RType::new(Mode::Null, Length::Zero);
        }
        let mut mode = Mode::Null;
        let mut total_len = Some(0usize);
        for t in arg_types {
            mode = mode.combine_result(if t.mode == Mode::Union {
                Mode::Opaque
            } else {
                t.mode
            });
            total_len = total_len.and_then(|total| match t.length {
                Length::Zero => Some(total),
                Length::One => total.checked_add(1),
                Length::Known(n) => total.checked_add(n),
                Length::Unknown => None,
            });
        }
        let length = if args
            .iter()
            .any(|a| matches!(a.value, Expr::Unknown(_) | Expr::Missing(_)))
        {
            Length::Unknown
        } else {
            total_len.map_or(Length::Unknown, Length::Known)
        };
        let result = RType::new(mode, length);
        if mode == Mode::Opaque {
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
        let length = Length::Known(arg_types.len());
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
        if args.iter().any(|arg| {
            matches!(&arg.value, Expr::Ident { name, .. } if name == "...")
                || matches!(&arg.value, Expr::Unknown(_) | Expr::Missing(_))
        }) {
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

    /// Infer `rep(x, times, each)`: length is `length(x) * times * each`
    /// with unsupplied counts defaulting to 1, keeping `x`'s mode, class,
    /// and schema. `length.out` takes precedence in R but is not modeled.
    /// `times`/`each` are read from the raw AST, not the inferred
    /// `RType`, because the type lattice discards the runtime value (a
    /// supplied non-literal means `Length::Unknown`). `x` is matched by
    /// name or first unnamed position, because named `times`/`each` can
    /// precede it in the call.
    pub(crate) fn infer_rep(&self, args: &[Arg], arg_types: &[RType]) -> RType {
        let x_type = find_arg(args, "x", 0)
            .and_then(|i| arg_types.get(i).cloned())
            .unwrap_or(RType::unknown());
        // Track `times` / `each` as `Option<Option<i64>>`:
        //   * outer None      -> not supplied (use default 1)
        //   * outer Some(None) -> supplied but non-literal (Unknown)
        //   * outer Some(Some(n)) -> supplied literal value n
        let times = find_arg(args, "times", 1)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
        let each = find_arg(args, "each", 2)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
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
            Length::Unknown => Length::Unknown,
        };
        RType { length, ..x_type }
    }

    /// Infer the result type of `seq(from, to, by)` / `seq.int(...)`.
    /// Literal forms pin the length exactly: `|to - from| / |by| + 1`
    /// (R rounds to the nearest whole step in range), `length.out = n`
    /// when supplied (it wins over `by`, as R documents), or
    /// `|to - from| + 1` when `by` is absent (R defaults it to +/-1).
    /// Otherwise the mode is still reported — integer when the first
    /// argument is an integer literal, else double — with
    /// `Length::Unknown`.
    pub(crate) fn infer_seq(&self, args: &[Arg], arg_types: &[RType]) -> RType {
        // Helper: find (was_supplied, literal_value) for a named or
        // positional argument. Named args win over positional. The
        // `pos` index counts only unnamed args, so `seq(from=1, 10)`
        // still matches `to` at positional index 0.
        let find = |name: &str, pos: usize| -> (bool, Option<i64>) {
            match find_arg(args, name, pos) {
                Some(i) => (true, extract_literal_int(&args[i].value)),
                None => (false, None),
            }
        };

        let (_, from_val) = find("from", 0);
        let (_, to_val) = find("to", 1);
        let (by_supplied, by_val) = find("by", 2);
        let (lo_supplied, lo_val) = find("length.out", 3);

        // Look at the named `from = ...` first, then the first
        // positional arg.
        let from_expr = args
            .iter()
            .find(|a| a.name.as_deref() == Some("from"))
            .or_else(|| args.iter().find(|a| a.name.is_none()))
            .map(|a| &a.value);
        let from_is_int_literal = from_expr
            .map(|e| matches!(e, Expr::Integer(_, _)))
            .unwrap_or(false);
        // Mode: integer if `from` is an integer literal or its inferred
        // type is integer, else double (mirrors the typeshed's
        // "double_or_int" rule).
        let mode =
            if from_is_int_literal || arg_types.first().map(|t| t.mode) == Some(Mode::Integer) {
                Mode::Integer
            } else {
                Mode::Double
            };

        // If a length-determining arg was supplied but wasn't a
        // literal, we can't pin the length. `length.out` and `by` both
        // participate in the length formula, so a non-literal value
        // for either forces Unknown. (`from`/`to` are handled below:
        // `extract_literal_int` returns None for them, which makes the
        // formula fall through to Unknown.)
        if (lo_supplied && lo_val.is_none()) || (by_supplied && by_val.is_none()) {
            return RType::new(mode, Length::Unknown);
        }

        // `length.out` wins over `by` when both are present.
        let length = if let Some(n) = lo_val {
            if n >= 0 {
                Length::Known(n as usize)
            } else {
                Length::Unknown
            }
        } else if let (Some(f), Some(t)) = (from_val, to_val) {
            match by_val {
                // by == 0: R errors at runtime; model as Unknown.
                Some(0) => Length::Unknown,
                Some(b) => {
                    let diff = (t - f).unsigned_abs() as usize;
                    let step = b.unsigned_abs() as usize;
                    Length::Known(diff / step + 1)
                }
                // by not supplied (the supplied-non-literal case
                // returned above): R defaults to +/-1.
                None => Length::Known((t - f).unsigned_abs() as usize + 1),
            }
        } else {
            Length::Unknown
        };
        RType::new(mode, length)
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
                let mut result = match slot {
                    ReturnSlot::Arg0 => first,
                    ReturnSlot::ConcatOfArgs => self.infer_c(args, arg_types),
                };
                if let Some(length) =
                    semantic_return_length(sig.return_length.as_ref(), &sig.params, args, arg_types)
                {
                    result.length = length;
                }
                result
            }
            ReturnSpec::Concrete(c) => {
                let mode = if let Some(mode) = concrete_json_mode(&c.mode) {
                    mode
                } else {
                    match JsonMode::parse(&c.mode) {
                        Some(JsonMode::Union) => {
                            return json_rtype_to_rtype(c);
                        }
                        // Compound specs that pick by arg type. For v1 we
                        // approximate "double_or_int" as the first arg's mode
                        // if it's already integer, else double.
                        Some(JsonMode::DoubleOrInt) => {
                            if matches!(first.mode, Mode::Integer) {
                                Mode::Integer
                            } else {
                                Mode::Double
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
                // and would build a malformed union. Collapse to opaque.
                let mode = if matches!(mode, Mode::Union) {
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
                let mut result = RType::new(mode, length);
                if c.class.is_empty() && mode == Mode::Opaque {
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

/// Find the argument for a named parameter: an exact-name argument wins, else
/// the `pos`-th unnamed (positional) argument. `pos` counts only unnamed
/// args, so `rep(each = 2, c(1,2,3), 1)` matches `x` at 0 and `times` at 1.
fn find_arg(args: &[Arg], name: &str, pos: usize) -> Option<usize> {
    for (i, a) in args.iter().enumerate() {
        if a.name.as_deref() == Some(name) {
            return Some(i);
        }
    }
    let mut idx = 0usize;
    for (i, a) in args.iter().enumerate() {
        if a.name.is_some() {
            continue;
        }
        if idx == pos {
            return Some(i);
        }
        idx += 1;
    }
    None
}

fn semantic_return_length(
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
                } else if !matches!(arg_types[index].mode, Mode::Opaque | Mode::Union) {
                    return Some(Length::One);
                } else {
                    return Some(Length::Unknown);
                }
            }
            if let Some(index) = bound_control(&spec.recycle0.param)
                && matches!(args[index].value, Expr::Logical(true, _))
                && value_types
                    .iter()
                    .any(|ty| matches!(ty.length, Length::Zero))
            {
                return Some(Length::Zero);
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
