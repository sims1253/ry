use super::*;
use crate::infer::*;
use crate::resolve::SchemaProvider;

impl Checker {
    /// Apply schema semantics declared by the resolved package signature.
    /// Resolution itself preserves package attachment, qualification,
    /// importFrom, and tidyverse gating.
    pub(crate) fn infer_schema_call(
        &mut self,
        name: &str,
        args: &[Arg],
        scope: &mut Scope,
    ) -> Option<RType> {
        let (sig, provider) = self.resolve_schema_sig(name)?;
        let effect = sig.schema_effect?;
        let data_index = data_mask_source_arg(&sig, args)?;
        let data_type = self.infer(&args[data_index].value, scope);
        let user_dispatch = self.resolve_user_s3_inherited_sig(name).is_some()
            || self.resolves_user_s3_dispatch(name, &data_type);
        // Base's declarative effects include with(list, ...),
        // subset(vector, ...), transform(), and within(). Their contracts
        // are not restricted to data frames. Package effects model standard
        // data-frame methods, whose custom dispatch may replace the result.
        let trusted_receiver = !user_dispatch
            && (provider == SchemaProvider::Base || standard_dplyr_frame(&data_type));
        let mut arg_types = vec![RType::unknown(); args.len()];
        arg_types[data_index] = data_type.clone();
        if matches!(effect, SchemaEffect::Join) {
            for (index, argument) in args.iter().enumerate() {
                if index != data_index {
                    arg_types[index] = self.infer(&argument.value, scope);
                }
            }
            if user_dispatch || (data_type.class.known && !trusted_receiver) {
                // A known custom class/method may replace join's result.
                return Some(RType::unknown());
            }
            // A receiver of unknown class still gets the join's column
            // facts, which later checks in the same pipeline use.
            let matched = match_args_to_params(&sig.params, args, &arg_types);
            return Some(infer_dplyr_join(&matched, args));
        }

        let mut local = self.dplyr_data_mask_scope(scope, &data_type);
        if user_dispatch {
            local = local.with_unknown_data_mask();
        }
        let mut named_results = Vec::new();
        let mut masked_args = Vec::new();
        let mut tidy_args = Vec::new();
        let mut uncertain_tag = false;
        let mut unsupported_control = false;
        for (index, argument) in args.iter().enumerate() {
            if index == data_index {
                continue;
            }
            let mode = argument_eval_mode(&sig, args, index).unwrap_or(EvalMode::Normal);
            let injection = argument_supports_injection(&sig, args, index);
            let inferred = match mode {
                EvalMode::Normal => self.infer(&argument.value, scope),
                EvalMode::DataMask => {
                    masked_args.push(argument);
                    local.insert(".", RType::unknown());
                    self.infer_with_injection(&argument.value, &mut local, injection)
                }
                EvalMode::TidySelect => {
                    if !argument.name.as_deref().is_some_and(|tag| {
                        schema_control_arg(effect, name, semantic_argument_name(tag))
                    }) {
                        tidy_args.push(argument);
                    }
                    self.infer_tidyselect_expr(&argument.value, &mut local, injection)
                }
                EvalMode::QuotedSymbol => {
                    if matches!(argument.value, Expr::Ident { .. }) {
                        RType::unknown()
                    } else {
                        self.infer_with_injection(&argument.value, &mut local, injection)
                    }
                }
                EvalMode::QuotedExpression | EvalMode::CapturesPromise => RType::unknown(),
            };
            if let Some(raw_name) = argument.name.as_deref() {
                uncertain_tag |= quoted_tag_has_escapes(raw_name);
                let column = semantic_argument_name(raw_name);
                unsupported_control |= effect == SchemaEffect::Transmute
                    && matches!(column, ".keep" | ".before" | ".after");
                if mode == EvalMode::DataMask && !schema_control_arg(effect, name, column) {
                    local.insert(column, inferred.clone());
                    local.mark_data_mask_column(column);
                    named_results.push((column, inferred.clone()));
                }
            }
            arg_types[index] = inferred;
        }

        if !trusted_receiver {
            // These effects describe standard data-frame methods, not an
            // arbitrary S3 override. An incomplete source schema still
            // asserts types for its named columns; discard those facts
            // before a custom method can make them false.
            return Some(RType::unknown());
        }

        let mut result = match effect {
            SchemaEffect::Preserve => data_type,
            SchemaEffect::AddNamedArgs => schema_mutated_type(
                data_type,
                &named_results,
                &masked_args,
                args,
                crate::semantic_lists::bare_name(name) == "mutate",
            ),
            SchemaEffect::GroupBy => {
                let source_grouped = data_type.class.contains("grouped_df");
                let source_columns = data_type.columns.clone();
                let mut result =
                    schema_mutated_type(data_type, &named_results, &masked_args, &[], false);
                let group_specs = args.iter().enumerate().filter(|(index, arg)| {
                    *index != data_index
                        && !arg.name.as_deref().is_some_and(|name| {
                            matches!(semantic_argument_name(name), ".add" | ".drop")
                        })
                });
                let mut has_group_spec = false;
                let mut possible_group_spec = false;
                for (index, arg) in group_specs {
                    let source_column = matches!(&arg.value, Expr::Ident { name, .. }
                        if source_columns.as_ref().is_some_and(|schema| schema.get(name).is_some()));
                    match arg_types[index].mode {
                        _ if source_column => has_group_spec = true,
                        Mode::Logical
                        | Mode::Integer
                        | Mode::Double
                        | Mode::Complex
                        | Mode::Character
                        | Mode::Raw => has_group_spec = true,
                        Mode::Null => {}
                        // Splices, across/pick, and unions that may be NULL
                        // can expand to zero grouping columns. Their type
                        // alone cannot certify a grouped result class.
                        _ => possible_group_spec = true,
                    }
                }
                let add = args
                    .iter()
                    .find(|arg| arg.name.as_deref().map(semantic_argument_name) == Some(".add"));
                let retain_groups = source_grouped
                    && add.is_some_and(|arg| matches!(arg.value, Expr::Logical(true, _)));
                let uncertain_groups = source_grouped
                    && add.is_some_and(|arg| !matches!(arg.value, Expr::Logical(_, _)));
                if has_group_spec || retain_groups {
                    result.class =
                        ClassVector::from_slice(&["grouped_df", "tbl_df", "tbl", "data.frame"]);
                } else if !uncertain_groups && !possible_group_spec {
                    result.class = ClassVector::from_slice(&["tbl_df", "tbl", "data.frame"]);
                } else {
                    result.class = ClassVector::unknown();
                }
                if uncertain_groups || possible_group_spec {
                    // A dynamic .add may preserve old grouping keys. Later
                    // summarise/transmute cannot certify their absence. A
                    // dynamic grouping spec may add groups or none.
                    result = incomplete_schema(result);
                }
                result
            }
            SchemaEffect::Transmute => {
                schema_transmuted_type(data_type, &named_results, &masked_args)
            }
            SchemaEffect::Rename => schema_renamed_type(data_type, &tidy_args, true, args),
            SchemaEffect::Relocate => schema_renamed_type(data_type, &tidy_args, false, args),
            SchemaEffect::Select => {
                // In base::subset(), `select` names the formal argument; it
                // does not rename the selected column. Package select()
                // effects may use named selectors as output names.
                schema_selected_type(data_type, &tidy_args, provider == SchemaProvider::Package)
            }
            SchemaEffect::Aggregate => {
                schema_aggregate_type(data_type, &named_results, &masked_args, args)
            }
            SchemaEffect::ExpressionValue => match_args_to_params(&sig.params, args, &arg_types)
                .get(1)
                .cloned()
                .unwrap_or_else(RType::unknown),
            SchemaEffect::Join => unreachable!("joins return before data-mask evaluation"),
            SchemaEffect::Pivot => RType::new(Mode::List, Length::Unknown)
                .with_class(ClassVector::single("data.frame")),
        };
        if uncertain_tag || unsupported_control {
            // Escaped tags and unsupported controls cannot prove a complete
            // result shape even for a standard data-frame receiver.
            result = incomplete_schema(result);
        }
        Some(result)
    }

    pub(crate) fn infer_tidyselect_expr(
        &mut self,
        expr: &Expr,
        scope: &mut Scope,
        injection: Option<InjectionMode>,
    ) -> RType {
        let previous = scope.tidy_injection;
        scope.tidy_injection = injection.max(previous);
        let result = match expr {
            Expr::String(_, _) => RType::scalar(Mode::Character),
            Expr::Ident { name, .. } => scope.get(name).cloned().unwrap_or_else(RType::unknown),
            Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            } => {
                let _ = self.infer_tidyselect_expr(expr, scope, None);
                RType::unknown()
            }
            Expr::Call { func, args, .. } if is_c_call_head(func) => {
                for a in args {
                    let _ = self.infer_tidyselect_expr(&a.value, scope, None);
                }
                RType::unknown()
            }
            _ => self.infer(expr, scope),
        };
        scope.tidy_injection = previous;
        result
    }

    /// Build the mask for `x[i, j]` index arguments on a data.table-shaped
    /// receiver (a `data.table` class, or the opaque value a data.table
    /// call types as; see `table_index_receiver`). Reuses the dplyr mask
    /// machinery so both mask families resolve free names the same way: a
    /// known schema's columns shadow lexical bindings and scope functions,
    /// and an unenumerable schema keeps bare symbols opaque instead of
    /// borrowing a scope function's type. On top of the shared
    /// pronouns, data.table's j position also binds `.SD` and friends;
    /// `.SD`'s columns depend on `by` and `.SDcols`, so only the table
    /// shape is retained.
    ///
    /// When the schema cannot prove a name absent (unknown, incomplete,
    /// or an opaque receiver), the mask's columns-first flag marks bare
    /// value-position symbols as column candidates before scope
    /// functions. A receiver that is opaque without any table evidence
    /// may equally be an atomic vector whose type degraded
    /// (`c(if (p) 1L, 2L)`), so its mask additionally stays eager for
    /// names: symbols that resolve nowhere keep RY010 like ordinary
    /// vector subsetting.
    pub(crate) fn table_index_mask_scope(&self, base_scope: &Scope, table: &RType) -> Scope {
        let mut scope = self.dplyr_data_mask_scope(base_scope, table);
        scope.insert(
            ".SD",
            RType {
                columns: None,
                ..table.clone()
            },
        );
        scope.insert(".N", RType::scalar(Mode::Integer));
        scope.insert(".I", RType::new(Mode::Integer, Length::Unknown));
        scope.insert(".BY", RType::unknown());
        scope.insert(".GRP", RType::scalar(Mode::Integer));
        if !complete_frame(table) {
            // Inside this table `[` mask, a bare value-position symbol is
            // a column candidate before it is a scope function.
            if let Some(frame) = scope.data_mask.as_mut() {
                Arc::make_mut(frame).columns_first = true;
            }
            if !frame_like(table) && table.columns.is_none() {
                scope.data_mask_unknown = base_scope.data_mask_unknown;
            }
        }
        scope
    }

    pub(crate) fn dplyr_data_mask_scope(&self, base_scope: &Scope, df_type: &RType) -> Scope {
        let enclosing = base_scope.data_mask.as_deref();
        let mut frame = DataMaskFrame {
            columns_first: enclosing.is_some_and(|frame| frame.columns_first),
            columns: enclosing
                .map(|frame| frame.columns.clone())
                .unwrap_or_default(),
            // Snapshot the lexical environment before columns are overlaid.
            env: base_scope.bindings.clone(),
            env_unknown: false,
        };
        let mut scope = match &df_type.columns {
            Some(schema) => {
                frame
                    .columns
                    .extend(schema.columns.iter().map(|(name, _)| name.clone()));
                scope_with_columns(base_scope, schema)
            }
            None => base_scope.independent_execution_scope(),
        };
        scope.data_mask = Some(Arc::new(frame));
        // Some selection APIs take a character vector of column names.
        // The pronoun represents a mask, never that input's atomic storage.
        let pronoun = if df_type.columns.is_some() && frame_like(df_type) {
            df_type.clone()
        } else {
            RType::unknown()
        };
        scope.insert(".data", pronoun);
        scope.insert(".env", RType::unknown());
        if !complete_frame(df_type) {
            scope = scope.with_unknown_data_mask();
        }
        scope
    }
}

fn frame_like(ty: &RType) -> bool {
    ty.class.contains("data.frame") || ty.mode == Mode::List
}

/// A frame whose schema enumerates every column, so it proves a name absent.
fn complete_frame(ty: &RType) -> bool {
    ty.columns.as_ref().is_some_and(|schema| schema.complete) && frame_like(ty)
}

fn incomplete_schema(mut data_type: RType) -> RType {
    if let Some(schema) = &data_type.columns {
        let mut schema = (**schema).clone();
        schema.complete = false;
        data_type.columns = Some(Arc::new(schema));
    }
    data_type
}

fn schema_control_arg(effect: SchemaEffect, call: &str, tag: &str) -> bool {
    let verb = crate::semantic_lists::bare_name(call);
    match effect {
        SchemaEffect::AddNamedArgs if verb == "mutate" => {
            matches!(tag, ".by" | ".keep" | ".before" | ".after")
        }
        SchemaEffect::GroupBy => matches!(tag, ".add" | ".drop"),
        // transmute() rejects these mutate controls before returning a frame.
        SchemaEffect::Transmute => matches!(tag, ".keep" | ".before" | ".after"),
        SchemaEffect::Aggregate if matches!(verb, "summarise" | "summarize") => {
            matches!(tag, ".by" | ".groups")
        }
        SchemaEffect::Aggregate if verb == "reframe" => tag == ".by",
        SchemaEffect::Relocate => matches!(tag, ".before" | ".after"),
        _ => false,
    }
}

fn quoted_tag_has_escapes(raw: &str) -> bool {
    raw.as_bytes()
        .first()
        .is_some_and(|quote| matches!(quote, b'\'' | b'"' | b'`'))
        && raw.contains('\\')
}

fn standard_dplyr_frame(data_type: &RType) -> bool {
    let class = &data_type.class;
    class.known
        && class.contains("data.frame")
        && class.names[..class.len as usize].iter().all(|name| {
            name.as_deref()
                .is_some_and(|name| matches!(name, "data.frame" | "tbl_df" | "tbl" | "grouped_df"))
        })
}

fn drop_column(mut data_type: RType, name: &str) -> RType {
    if let Some(schema) = &data_type.columns {
        let mut schema = (**schema).clone();
        schema.columns.retain(|(column, _)| column != name);
        data_type.columns = Some(Arc::new(schema));
    }
    data_type
}

/// A named data-mask result replaces its column; a NULL result drops it.
fn assign_named_results(mut data_type: RType, named: &[(&str, RType)]) -> RType {
    for (name, ty) in named {
        data_type = if ty.mode == Mode::Null {
            drop_column(data_type, name)
        } else {
            type_with_assigned_column(data_type, name, ty.clone())
        };
    }
    data_type
}

/// An unnamed bare symbol naming an atomic source column, which data-mask
/// verbs keep under its own name.
fn passthrough_column<'a>(arg: &'a Arg, source: &RType) -> Option<(&'a str, RType)> {
    let Expr::Ident { name, .. } = &arg.value else {
        return None;
    };
    let ty = source.columns.as_ref()?.get(name)?;
    (arg.name.is_none() && name != "..." && ty.columns.is_none()).then_some((name, ty))
}

fn masked_output_is_dynamic(args: &[&Arg], source: &RType) -> bool {
    args.iter()
        .any(|arg| arg.name.is_none() && passthrough_column(arg, source).is_none())
}

fn selected_columns_from_source(data_type: &RType, expr: &Expr) -> Option<Vec<(String, RType)>> {
    let source = data_type.columns.as_ref()?;
    let mut operations = Vec::new();
    if !collect_ordered_tidy_selection(expr, false, false, &mut operations) {
        return None;
    }
    apply_tidy_selection(source, operations)
}

/// Apply ordered `(exclude, source column, output name)` selections.
/// Tidyselect begins with all columns only when the first selector is
/// negative, so an empty c() selects nothing. Operations apply in order:
/// c(-x, x) removes x and then reintroduces it. A named selector renames the
/// column's unrenamed entry in place, otherwise it adds another output, so
/// `a = x, b = x` keeps both. A bare selector of an already selected column
/// is a no-op. `None` when a selector names a column the schema lacks or
/// reuses an output name for another column.
fn apply_tidy_selection(
    source: &ColumnSchema,
    operations: Vec<(bool, String, String)>,
) -> Option<Vec<(String, RType)>> {
    let mut selected: Vec<(String, String, RType)> =
        if matches!(operations.first(), Some((true, ..))) {
            source
                .columns
                .iter()
                .map(|(name, ty)| (name.clone(), name.clone(), ty.clone()))
                .collect()
        } else {
            Vec::new()
        };
    for (exclude, name, output) in operations {
        let ty = source.get(&name)?;
        if exclude {
            selected.retain(|(_, selected_source, _)| *selected_source != name);
        } else if output == name {
            if !selected.iter().any(|(_, source, _)| *source == name) {
                selected.push((output, name, ty));
            }
        } else if let Some(entry) = selected
            .iter_mut()
            .find(|(selected_output, source, _)| *source == name && *selected_output == name)
        {
            entry.0 = output;
        } else if let Some((_, source, _)) = selected.iter().find(|(o, ..)| *o == output) {
            if *source != name {
                return None;
            }
        } else {
            selected.push((output, name, ty));
        }
    }
    Some(
        selected
            .into_iter()
            .map(|(output, _, ty)| (output, ty))
            .collect(),
    )
}

fn collect_ordered_tidy_selection(
    expr: &Expr,
    excluded: bool,
    inside_combine: bool,
    operations: &mut Vec<(bool, String, String)>,
) -> bool {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => {
            operations.push((excluded, name.clone(), name.clone()));
            true
        }
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
            ..
        } if !excluded => collect_ordered_tidy_selection(expr, true, inside_combine, operations),
        Expr::Call { func, args, .. } if !inside_combine && is_c_call_head(func) => {
            args.iter().all(|arg| {
                arg.name.is_none()
                    && collect_ordered_tidy_selection(&arg.value, excluded, true, operations)
            })
        }
        _ => false,
    }
}

fn schema_mutated_type(
    mut data_type: RType,
    named: &[(&str, RType)],
    masked_args: &[&Arg],
    args: &[Arg],
    mutate_controls: bool,
) -> RType {
    let source = data_type.clone();
    if let Some(keep) = args.iter().find(|arg| {
        mutate_controls && arg.name.as_deref().map(semantic_argument_name) == Some(".keep")
    }) {
        match &keep.value {
            Expr::String(value, _) if value == "all" => {}
            Expr::String(value, _) if value == "none" => {
                let complete = data_type
                    .columns
                    .as_ref()
                    .is_some_and(|schema| schema.complete)
                    && !data_type.class.contains("grouped_df");
                let mut schema = ColumnSchema {
                    complete,
                    ..ColumnSchema::default()
                };
                if let Some(by) = args
                    .iter()
                    .find(|arg| arg.name.as_deref().map(semantic_argument_name) == Some(".by"))
                {
                    if let Some(columns) = selected_columns_from_source(&source, &by.value) {
                        schema.columns = columns;
                    } else {
                        schema.complete = false;
                    }
                }
                data_type.columns = Some(Arc::new(schema));
            }
            // "used" and "unused" depend on reads within expressions.
            _ => data_type = incomplete_schema(data_type),
        }
    }
    if mutate_controls {
        for (name, ty) in masked_args
            .iter()
            .filter_map(|arg| passthrough_column(arg, &source))
        {
            data_type = type_with_assigned_column(data_type, name, ty);
        }
    }
    data_type = assign_named_results(data_type, named);
    if masked_output_is_dynamic(masked_args, &source) {
        data_type = incomplete_schema(data_type);
    }
    data_type
}

fn schema_transmuted_type(
    mut data_type: RType,
    named: &[(&str, RType)],
    masked_args: &[&Arg],
) -> RType {
    let source_type = data_type.clone();
    let source = data_type.columns.clone();
    data_type.columns = Some(Arc::new(ColumnSchema {
        complete: source.as_ref().is_some_and(|schema| schema.complete)
            && !data_type.class.contains("grouped_df"),
        ..ColumnSchema::default()
    }));
    for arg in masked_args {
        if arg.name.is_none()
            && let Expr::Ident { name, .. } = &arg.value
        {
            if let Some(ty) = source.as_ref().and_then(|schema| schema.get(name)) {
                data_type = type_with_assigned_column(data_type, name, ty);
            } else {
                data_type = incomplete_schema(data_type);
            }
        }
    }
    data_type = assign_named_results(data_type, named);
    if masked_output_is_dynamic(masked_args, &source_type) {
        data_type = incomplete_schema(data_type);
    }
    data_type
}

fn schema_renamed_type(
    mut data_type: RType,
    args: &[&Arg],
    require_names: bool,
    all_args: &[Arg],
) -> RType {
    let Some(source) = data_type.columns.as_ref() else {
        return data_type;
    };
    let mut schema = (**source).clone();
    let mut selected = Vec::new();
    for arg in args {
        let Some(old_name) = binding_name(&arg.value) else {
            schema.complete = false;
            continue;
        };
        let Some(index) = source.columns.iter().position(|(name, _)| name == old_name) else {
            schema.complete = false;
            continue;
        };
        if selected.contains(&index) {
            schema.complete = false;
            continue;
        }
        selected.push(index);
        if let Some(raw_name) = arg.name.as_deref() {
            // All selectors name columns in the original input. Looking up
            // the already-renamed output would swap types for y=x, z=y.
            schema.columns[index].0 = semantic_argument_name(raw_name).to_owned();
        } else if require_names {
            schema.complete = false;
        }
    }
    if !require_names {
        if all_args.iter().any(|arg| {
            matches!(
                arg.name.as_deref().map(semantic_argument_name),
                Some(".before" | ".after")
            )
        }) {
            // Position controls can use arbitrary tidyselect expressions.
            schema.complete = false;
        } else if !selected.is_empty() {
            let mut columns = selected
                .iter()
                .map(|index| schema.columns[*index].clone())
                .collect::<Vec<_>>();
            columns.extend(
                schema
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !selected.contains(index))
                    .map(|(_, column)| column.clone()),
            );
            schema.columns = columns;
        }
    }
    if schema.columns.iter().enumerate().any(|(index, (name, _))| {
        schema.columns[..index]
            .iter()
            .any(|(prior, _)| prior == name)
    }) {
        schema.complete = false;
    }
    data_type.columns = Some(Arc::new(schema));
    data_type
}

fn schema_selected_type(data_type: RType, args: &[&Arg], rename_named: bool) -> RType {
    if args.is_empty() {
        return data_type;
    }
    let Some(schema) = data_type.columns.clone() else {
        return data_type;
    };
    let mut operations = Vec::new();
    for arg in args {
        let collected = match arg.name.as_deref().filter(|_| rename_named) {
            Some(output) => binding_name(&arg.value).map(|source| {
                let output = semantic_argument_name(output).to_owned();
                operations.push((false, source.to_owned(), output));
            }),
            None => collect_ordered_tidy_selection(&arg.value, false, false, &mut operations)
                .then_some(()),
        };
        if collected.is_none() {
            return incomplete_schema(data_type);
        }
    }
    let Some(columns) = apply_tidy_selection(&schema, operations) else {
        return incomplete_schema(data_type);
    };
    data_type.with_columns(Arc::new(ColumnSchema {
        columns,
        complete: schema.complete,
        locally_constructed: false,
    }))
}

fn schema_aggregate_type(
    data_type: RType,
    named: &[(&str, RType)],
    masked_args: &[&Arg],
    args: &[Arg],
) -> RType {
    let mut schema = ColumnSchema {
        complete: data_type
            .columns
            .as_ref()
            .is_some_and(|source| source.complete)
            && !data_type.class.contains("grouped_df")
            && !masked_output_is_dynamic(masked_args, &data_type),
        ..ColumnSchema::default()
    };
    if let Some(by) = args
        .iter()
        .find(|arg| arg.name.as_deref().map(semantic_argument_name) == Some(".by"))
    {
        if let Some(columns) = selected_columns_from_source(&data_type, &by.value) {
            schema.columns.extend(columns);
        } else {
            schema.complete = false;
        }
    }
    let class = if data_type.class.contains("grouped_df") {
        ClassVector::unknown()
    } else if data_type.class.contains("tbl_df") {
        ClassVector::from_slice(&["tbl_df", "tbl", "data.frame"])
    } else {
        ClassVector::single("data.frame")
    };
    let mut result = RType::new(Mode::List, Length::One)
        .with_class(class)
        .with_columns(Arc::new(schema));
    for arg in masked_args.iter().filter(|arg| arg.name.is_none()) {
        result = match passthrough_column(arg, &data_type) {
            Some((name, ty)) => type_with_assigned_column(result, name, ty),
            None => incomplete_schema(result),
        };
    }
    assign_named_results(result, named)
}

/// Whether a column `x` and `y` share is a join key, which appears once.
/// Without `by`, a natural join uses every shared column as a key. Unknown
/// keys, `suffix`, and `keep` leave every shared column possibly renamed.
fn shared_join_key(args: &[Arg]) -> impl Fn(&str) -> bool {
    fn name(arg: &Arg) -> Option<&str> {
        arg.name.as_deref().map(semantic_argument_name)
    }
    // Unnamed arguments fill whichever of `x` and `y` were not named; any
    // further unnamed argument is a positional option.
    let mut positional_tables = 2usize.saturating_sub(
        args.iter()
            .filter(|arg| matches!(name(arg), Some("x" | "y")))
            .count(),
    );
    let mut by = None;
    let mut known = true;
    for arg in args {
        match name(arg) {
            Some("by") => by = Some(&arg.value),
            Some("suffix" | "keep") => known = false,
            Some(_) => {}
            None if positional_tables > 0 => positional_tables -= 1,
            None => known = false,
        }
    }
    let keys: Option<Vec<String>> = match by {
        _ if !known => Some(Vec::new()),
        None => None,
        Some(Expr::String(name, _)) => Some(vec![name.clone()]),
        Some(Expr::Call { func, args, .. }) if is_c_call_head(func) => Some(
            args.iter()
                .map(|arg| match (&arg.name, &arg.value) {
                    (None, Expr::String(name, _)) => Some(name.clone()),
                    _ => None,
                })
                .collect::<Option<_>>()
                .unwrap_or_default(),
        ),
        Some(_) => Some(Vec::new()),
    };
    move |name| {
        keys.as_ref()
            .is_none_or(|keys| keys.iter().any(|key| key == name))
    }
}

fn infer_dplyr_join(arg_types: &[RType], args: &[Arg]) -> RType {
    let x_type = arg_types.first().cloned().unwrap_or_else(RType::unknown);
    let y_type = arg_types.get(1).cloned().unwrap_or_else(RType::unknown);
    let mut result =
        RType::new(Mode::List, Length::Unknown).with_class(ClassVector::single("data.frame"));

    let mut columns = Vec::new();
    let mut complete = true;
    if let Some(schema) = &x_type.columns {
        columns.extend(schema.columns.iter().cloned());
        complete &= schema.complete;
    } else {
        complete = false;
    }
    if let Some(schema) = &y_type.columns {
        let is_key = shared_join_key(args);
        let mut suffixed = Vec::new();
        for (name, ty) in &schema.columns {
            if !columns.iter().any(|(existing, _)| existing == name) {
                columns.push((name.clone(), ty.clone()));
            } else if !is_key(name) {
                suffixed.push(name);
            }
        }
        // A shared non-key column appears as `v.x` and `v.y` (or under
        // custom suffixes), never as `v`.
        if !suffixed.is_empty() {
            columns.retain(|(name, _)| !suffixed.contains(&name));
            complete = false;
        }
        complete &= schema.complete;
    } else {
        complete = false;
    }

    if !columns.is_empty() {
        result = result.with_columns(Arc::new(ColumnSchema {
            columns,
            complete,
            locally_constructed: false,
        }));
    }
    result
}

fn scope_with_columns(base_scope: &Scope, schema: &Arc<ColumnSchema>) -> Scope {
    let mut scope = base_scope.function_execution_scope();
    for (name, ty) in &schema.columns {
        scope.insert(name.clone(), ty.clone());
    }
    scope
}
