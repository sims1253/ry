use super::*;

impl Checker {
    pub(crate) fn infer_c(&mut self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
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
                    return RType::new(collapse_c_mode(mode, saw_union), Length::Unknown);
                }
            });
        }
        let length = if args.iter().any(|a| matches!(a.value, Expr::Unknown(_))) {
            Length::Unknown
        } else {
            Length::Known(total_len)
        };
        RType::new(collapse_c_mode(mode, saw_union), length)
    }

    // Infer the type of `list(...)`. The result is always a list whose
    // length equals the argument count; if at least one argument is
    // named, we additionally build a column schema from the named
    // args (positional args get R's auto-generated `[[i]]` names).
    //
    // We build the schema even when only some args are named: that
    // mirrors R's `list(a = 1, "x")` which produces names `c("a", "2")`.
    // The schema is what powers `df$col` / `df[["col"]]` resolution
    // downstream.
    pub(crate) fn infer_list(&mut self, arg_types: &[RType], args: &[Arg], _span: Span) -> RType {
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

    // Infer the type of `data.frame(...)`. Same column-schema logic as
    // `list(...)`, but:
    // * The result is classed `"data.frame"`.
    // * Column lengths are coerced to a common length (R recycles). For
    //   v1 we take the max of the known lengths (or Unknown if any
    //   column's length is Unknown), and propagate that length onto
    //   each column so `df$col` returns a vector of the right length.
    // * Special arguments like `row.names = ...`, `check.names = ...`
    //   are NOT columns and are dropped from the schema. We recognize
    //   the common ones by name.
    pub(crate) fn infer_data_frame(
        &mut self,
        arg_types: &[RType],
        args: &[Arg],
        _span: Span,
    ) -> RType {
        // Filter out non-column named arguments first. Positional args
        // are kept (they become columns); known metadata args are dropped
        // so they don't pollute the schema.
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

        // Compute the common column length (max of known lengths).
        let mut common_len: Length = Length::One;
        for t in &filtered_types {
            common_len = match (common_len, t.length) {
                (Length::Zero, x) | (x, Length::Zero) => x,
                (Length::One, x) | (x, Length::One) => x,
                (Length::Known(a), Length::Known(b)) => Length::Known(a.max(b)),
                _ => Length::Unknown,
            };
        }

        // Build per-column types with the coerced length.
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
        let mut schema = build_data_frame_schema(&coerced_types, &filtered_args);
        if let Some(s) = schema.as_mut() {
            // Sanity: lengths should already match coerced_types.
            debug_assert_eq!(s.columns.len(), coerced_types.len());
        }

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

    pub(crate) fn infer_rep(&self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        // Infer `rep(x, times, each)`: length = `length(x) * times * each`
        // with `times`/`each` defaulting to 1, and the result keeping
        // `x`'s mode, class, and schema (so `rep(factor(...), 3)` stays
        // a factor). `length.out` is not modeled: in R it takes
        // precedence over `times`, but the length here ignores it.
        //
        // `times`/`each` come from the raw AST, not the inferred
        // `RType`, because the type lattice discards the runtime value:
        // a supplied non-literal means `Length::Unknown`; an unsupplied
        // one defaults to 1.
        //
        // `find_idx`'s `pos` counts only unnamed args, so
        // `rep(each = 2, c(1,2,3), 1)` matches `x` at 0 and `times`
        // at 1 (mirrors `infer_seq`).
        let find_idx = |name: &str, pos: usize| -> Option<usize> {
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
        };
        // `x` is the first positional arg (pos 0) or a named `x = ...`.
        // We must look it up by index rather than `arg_types.first()`
        // because named `times`/`each` args can precede `x` in the
        // call (e.g. `rep(each = 2, c(1,2,3), 1)`).
        let x_type = find_idx("x", 0)
            .and_then(|i| arg_types.get(i).cloned())
            .unwrap_or(RType::unknown());
        // Track `times` / `each` as `Option<Option<i64>>`:
        //   * outer None      -> not supplied (use default 1)
        //   * outer Some(None) -> supplied but non-literal (Unknown)
        //   * outer Some(Some(n)) -> supplied literal value n
        let times = find_idx("times", 1)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
        let each = find_idx("each", 2)
            .and_then(|i| args.get(i))
            .map(|a| extract_literal_int(&a.value));
        // Resolve `times`. Non-supplied -> 1; non-literal -> Unknown;
        // negative literal -> Unknown (R errors or recycles in ways we
        // can't model, so we stay conservative rather than pin a wrong
        // length).
        let times_n: usize = match times {
            None => 1usize,
            Some(Some(n)) if n < 0 => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                };
            }
            Some(Some(n)) => n as usize,
            Some(None) => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                };
            }
        };
        let each_n: usize = match each {
            None => 1usize,
            Some(Some(n)) if n < 0 => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                };
            }
            Some(Some(n)) => n as usize,
            Some(None) => {
                return RType {
                    length: Length::Unknown,
                    ..x_type
                };
            }
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

    // Infer the result type of `seq(from, to, by)` / `seq.int(...)`.
    // Two literal forms let us pin the result length exactly:
    //   * `seq(from, to, by)`: length = `|to - from| / |by| + 1`
    //     (R rounds to the nearest whole step that stays in range).
    //   * `seq(from, to, length.out = n)`: length = `n`.
    //   * `seq(from, to)` (no `by`, no `length.out`): R defaults
    //     `by` to +/-1, so length = `|to - from| + 1`.
    //
    // When `length.out` is present it wins (R documents this as
    // taking precedence over `by`). When we can't pin the length, we
    // still report the right mode (integer when the first arg is an
    // integer literal, else double) with `Length::Unknown`.
    pub(crate) fn infer_seq(&self, args: &[Arg], arg_types: &[RType], _span: Span) -> RType {
        // Helper: find (was_supplied, literal_value) for a named or
        // positional argument. Named args win over positional. The
        // `pos` index counts only unnamed args, so `seq(from=1, 10)`
        // still matches `to` at positional index 0.
        let find = |name: &str, pos: usize| -> (bool, Option<i64>) {
            for a in args.iter() {
                if a.name.as_deref() == Some(name) {
                    return (true, extract_literal_int(&a.value));
                }
            }
            let mut idx = 0;
            for a in args.iter() {
                if a.name.is_some() {
                    continue;
                }
                if idx == pos {
                    return (true, extract_literal_int(&a.value));
                }
                idx += 1;
            }
            (false, None)
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
        _name: &str,
        sig: &FunctionSig,
        arg_types: &[RType],
        args: &[Arg],
        span: Span,
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
                    ReturnSlot::ConcatOfArgs => self.infer_c(args, arg_types, span),
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
                    Some(JsonLength::Known(0)) => Length::Zero,
                    Some(JsonLength::Known(1)) => Length::One,
                    Some(JsonLength::Known(value)) => Length::Known(value),
                    Some(JsonLength::Unknown) => Length::Unknown,
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
                    None => Length::Unknown,
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
                        .map(|(name, child)| (name.clone(), json_rtype_to_rtype_shallow(child)))
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
    let bindings = match_arguments(
        &signature_params
            .iter()
            .map(|param| param.name.as_str())
            .collect::<Vec<_>>(),
        args,
    );
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
