use super::*;

impl Checker {
    /// The class-constructor stage of `infer_call`: `structure(x, class =
    /// ...)`, `factor(x)`, and S4 `new("Class", ...)` attach a class to a
    /// payload value.
    pub(crate) fn infer_class_constructor_call(
        &mut self,
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
        if semantic_name == "structure" {
            return Some(self.infer_structure_call(args, scope));
        }
        // `factor(x)` returns an integer vector with class "factor".
        // (And often also "ordered" if `ordered = TRUE`, but we keep v1
        // to the base case.)
        if semantic_name == "factor" {
            // Infer args so unbound-variable diagnostics still fire.
            self.infer_args_for_diagnostics(args, scope);
            return Some(
                RType::new(Mode::Integer, Length::Unknown)
                    .with_class(ClassVector::single("factor")),
            );
        }
        if lookup_name == "new" {
            for argument in args.iter().skip(1) {
                let _ = self.infer(&argument.value, scope);
            }
            return Some(
                args.first()
                    .and_then(|argument| match &argument.value {
                        Expr::String(class, _) => {
                            Some(RType::unknown().with_class(ClassVector::single(class)))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(RType::unknown),
            );
        }
        None
    }

    /// Infer the type of `structure(x, class = "...")`. We model only
    /// the literal class forms; everything else returns the first
    /// argument's type with `ClassVector::unknown()` (so we neither lie
    /// about a class nor spuriously trigger RY050). The base value is the
    /// first positional or `x =` argument; later candidates are inferred
    /// for diagnostics only.
    ///
    /// The base value's column schema is preserved: `RType::with_class`
    /// is `RType { class, ..self }`, so a `structure(list(a = 1L),
    /// class = "foo")` call yields a value whose columns are still
    /// `[("a", integer<1>)]` and whose class is `["foo"]`. This lets
    /// `$a` resolve correctly on user-defined classes built on top of
    /// a list-shaped payload.
    pub(crate) fn infer_structure_call(&mut self, args: &[Arg], scope: &mut Scope) -> RType {
        let mut base_type = RType::unknown();
        let mut class_expr: Option<&Expr> = None;
        for a in args {
            if matches!(a.name.as_deref(), Some("class")) {
                class_expr = Some(&a.value);
                continue;
            }
            let is_base = matches!(a.name.as_deref(), None | Some("x"))
                && matches!(base_type.mode, Mode::Opaque);
            if is_base {
                base_type = self.infer(&a.value, scope);
            } else {
                let _ = self.infer(&a.value, scope);
            }
        }
        if let Some(ce) = class_expr {
            match parse_class_literal(ce) {
                ClassLiteral::Single(name) => {
                    return base_type.with_class(ClassVector::single(&name));
                }
                ClassLiteral::Multi(names) => {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    return base_type.with_class(ClassVector::from_slice(&refs));
                }
                ClassLiteral::Unknown => {
                    // Class is dynamic; keep base type but mark class as
                    // undetermined so RY050 stays quiet.
                    return base_type.with_class(ClassVector::unknown());
                }
            }
        }
        base_type
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
        let mut total_len: usize = 0;
        // A union arg would win the coerce-rank ladder and leave `mode ==
        // Union`, which `RType::new` then turns into a malformed union.
        // Track it and degrade to opaque at the end.
        let mut saw_union = false;
        for t in arg_types {
            if matches!(t.mode, Mode::Union) {
                saw_union = true;
                continue;
            }
            mode = mode.combine_result(t.mode);
            total_len = total_len.saturating_add(match t.length {
                Length::Zero => 0,
                Length::One => 1,
                Length::Known(n) => n,
                Length::Unknown => {
                    return RType::new(
                        if saw_union { Mode::Opaque } else { mode },
                        Length::Unknown,
                    );
                }
            });
        }
        let length = if args.iter().any(|a| matches!(a.value, Expr::Unknown(_))) {
            Length::Unknown
        } else {
            Length::Known(total_len)
        };
        RType::new(if saw_union { Mode::Opaque } else { mode }, length)
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
                || matches!(&arg.value, Expr::Unknown(_))
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
        let mode_expr = args
            .iter()
            .find(|a| a.name.as_deref() == Some("mode"))
            .or_else(|| args.iter().find(|a| a.name.is_none()))
            .map(|a| &a.value);
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

        let length_expr = args
            .iter()
            .find(|a| a.name.as_deref() == Some("length"))
            .or_else(|| {
                let mut positional = args.iter().filter(|a| a.name.is_none());
                let _ = positional.next();
                positional.next()
            })
            .map(|a| &a.value);
        let length = length_expr
            .and_then(extract_literal_int)
            .map(|n| {
                if n <= 0 {
                    Length::Zero
                } else {
                    Length::Known(n as usize)
                }
            })
            .unwrap_or(Length::Unknown);

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
