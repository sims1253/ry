use super::*;
use crate::infer::*;
use crate::resolve::SchemaProvider;

pub(crate) const DATA_MASK_ACTIVE: &str = "\0ry_data_mask";
pub(crate) const DATA_MASK_ENV_PREFIX: &str = "\0ry_data_mask_env:";
pub(crate) const DATA_MASK_COLUMN_PREFIX: &str = "\0ry_data_mask_column:";
pub(crate) const DATA_MASK_COLUMNS_FIRST: &str = "\0ry_data_mask_columns_first";

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
            // Keep the existing incomplete join modeling for a receiver
            // with unknown class: it carries source column facts used by
            // checks later in the same pipeline.
            let matched = match_args_to_params(&sig.params, args, &arg_types);
            return Some(infer_dplyr_join(&matched));
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
                    local.insert(
                        format!("{DATA_MASK_COLUMN_PREFIX}{column}"),
                        RType::unknown(),
                    );
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
                let mut result =
                    schema_mutated_type(data_type, &named_results, &masked_args, &[], false);
                let group_specs = args.iter().enumerate().filter(|(index, arg)| {
                    *index != data_index
                        && !arg.name.as_deref().is_some_and(|name| {
                            matches!(semantic_argument_name(name), ".add" | ".drop")
                        })
                });
                let mut has_group_spec = false;
                let mut forwarded_group_spec = false;
                for (_, arg) in group_specs {
                    match &arg.value {
                        Expr::Null(_) => {}
                        Expr::Call { func, args, .. }
                            if args.is_empty()
                                && ident_name(func).is_some_and(|name| {
                                    crate::semantic_lists::bare_name(name) == "c"
                                }) => {}
                        Expr::Ident { name, .. } if name == "..." => {
                            forwarded_group_spec = true;
                        }
                        _ => has_group_spec = true,
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
                } else if !uncertain_groups && !forwarded_group_spec {
                    result.class = ClassVector::from_slice(&["tbl_df", "tbl", "data.frame"]);
                } else {
                    result.class = ClassVector::unknown();
                }
                if uncertain_groups || forwarded_group_spec {
                    // A dynamic .add may preserve old grouping keys. Later
                    // summarise/transmute cannot certify their absence. A
                    // forwarded ... may add groups, or contain no arguments.
                    result = incomplete_schema(result);
                }
                result
            }
            SchemaEffect::Transmute => {
                schema_transmuted_type(data_type, &named_results, &masked_args)
            }
            SchemaEffect::Rename => schema_renamed_type(data_type, &tidy_args, true, args),
            SchemaEffect::Relocate => schema_renamed_type(data_type, &tidy_args, false, args),
            SchemaEffect::Select => schema_selected_type(data_type, &tidy_args),
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
            Expr::Call { func, args, .. }
                if ident_name(func)
                    .is_some_and(|name| crate::semantic_lists::bare_name(name) == "c") =>
            {
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
    /// borrowing a scope function's type (#369). On top of the shared
    /// pronouns, data.table's j position also binds `.SD` and friends;
    /// `.SD`'s columns depend on `by` and `.SDcols`, so only the table
    /// shape is retained.
    ///
    /// When the schema cannot prove a name absent (unknown, incomplete,
    /// or an opaque receiver), the columns-first sentinel marks bare
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
        let schema_proves_absence = table.columns.as_ref().is_some_and(|schema| schema.complete)
            && (table.class.contains("data.frame") || matches!(table.mode, Mode::List));
        if !schema_proves_absence {
            // #369: inside this table `[` mask, a bare value-position
            // symbol is a column candidate before it is a scope function.
            scope.insert(DATA_MASK_COLUMNS_FIRST, RType::unknown());
            let carries_table_evidence = table.class.contains("data.frame")
                || table.columns.is_some()
                || matches!(table.mode, Mode::List);
            if !carries_table_evidence {
                scope.data_mask_unknown = base_scope.data_mask_unknown;
            }
        }
        scope
    }

    pub(crate) fn dplyr_data_mask_scope(&self, base_scope: &Scope, df_type: &RType) -> Scope {
        // Keep a private snapshot of the lexical environment before columns
        // are overlaid. `.env$x` and `{{ x }}` must bypass data-mask column
        // shadowing and resolve here instead.
        let lexical_bindings: Vec<_> = base_scope
            .bindings
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect();
        let mut scope = match &df_type.columns {
            Some(schema) => scope_with_columns(base_scope, schema),
            None => base_scope.independent_execution_scope(),
        };
        scope.insert(DATA_MASK_ACTIVE, RType::unknown());
        // Some selection APIs take a character vector of column names.
        // The pronoun represents a mask, never that input's atomic storage.
        let pronoun = if df_type.columns.is_some()
            && (df_type.mode == Mode::List || df_type.class.contains("data.frame"))
        {
            df_type.clone()
        } else {
            RType::unknown()
        };
        scope.insert(".data", pronoun);
        scope.insert(".env", RType::unknown());
        for (name, ty) in lexical_bindings {
            scope.insert(format!("{DATA_MASK_ENV_PREFIX}{name}"), ty);
        }
        let schema_is_complete = df_type
            .columns
            .as_ref()
            .map(|schema| {
                schema.complete
                    && (df_type.class.contains("data.frame") || matches!(df_type.mode, Mode::List))
            })
            .unwrap_or(false);
        if !schema_is_complete {
            scope = scope.with_unknown_data_mask();
        }
        scope
    }
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

fn masked_output_is_dynamic(args: &[&Arg], source: &RType) -> bool {
    args.iter().any(|arg| {
        arg.name.is_none()
            && !matches!(
                &arg.value,
                Expr::Ident { name, .. }
                    if name != "..."
                        && source.columns.as_ref().is_some_and(|schema| {
                            schema.get(name).is_some_and(|ty| ty.columns.is_none())
                        })
            )
    })
}

fn selected_columns_from_source(data_type: &RType, expr: &Expr) -> Option<Vec<(String, RType)>> {
    let source = data_type.columns.as_ref()?;
    let mut operations = Vec::new();
    if !collect_ordered_tidy_selection(expr, false, false, &mut operations) {
        return None;
    }
    // Tidyselect begins with all columns only when the first selector is
    // negative. An empty c() therefore selects nothing. Apply operations
    // in order: c(-x, x) removes x and then reintroduces it.
    let mut selected = if matches!(operations.first(), Some((true, _))) {
        source.columns.clone()
    } else {
        Vec::new()
    };
    for (exclude, name) in operations {
        let column = source
            .columns
            .iter()
            .find(|(source_name, _)| *source_name == name)?;
        if exclude {
            selected.retain(|(selected_name, _)| selected_name != &name);
        } else if !selected
            .iter()
            .any(|(selected_name, _)| selected_name == &name)
        {
            selected.push(column.clone());
        }
    }
    Some(selected)
}

fn collect_ordered_tidy_selection(
    expr: &Expr,
    excluded: bool,
    inside_combine: bool,
    operations: &mut Vec<(bool, String)>,
) -> bool {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => {
            operations.push((excluded, name.clone()));
            true
        }
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
            ..
        } if !excluded => collect_ordered_tidy_selection(expr, true, inside_combine, operations),
        Expr::Call { func, args, .. }
            if !inside_combine
                && ident_name(func)
                    .is_some_and(|name| crate::semantic_lists::bare_name(name) == "c") =>
        {
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
        for arg in masked_args.iter().filter(|arg| arg.name.is_none()) {
            if let Expr::Ident { name, .. } = &arg.value
                && let Some(ty) = source.columns.as_ref().and_then(|schema| schema.get(name))
                && ty.columns.is_none()
            {
                data_type = type_with_assigned_column(data_type, name, ty);
            }
        }
    }
    for (name, ty) in named {
        data_type = if ty.mode == Mode::Null {
            drop_column(data_type, name)
        } else {
            type_with_assigned_column(data_type, name, ty.clone())
        };
    }
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
    for (name, ty) in named {
        data_type = if ty.mode == Mode::Null {
            drop_column(data_type, name)
        } else {
            type_with_assigned_column(data_type, name, ty.clone())
        };
    }
    if masked_output_is_dynamic(masked_args, &source_type) {
        data_type = incomplete_schema(data_type);
    }
    data_type
}

fn selected_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => Some(name),
        _ => None,
    }
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
        let Some(old_name) = selected_name(&arg.value) else {
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

fn schema_selected_type(mut data_type: RType, args: &[&Arg]) -> RType {
    if args.is_empty() {
        return data_type;
    }
    let Some(schema) = data_type.columns.as_ref() else {
        return data_type;
    };
    let mut includes: Vec<(String, String)> = Vec::new();
    let mut excludes = Vec::new();
    for arg in args {
        if let Some(name) = arg.name.as_deref() {
            let Some(old_name) = selected_name(&arg.value) else {
                return incomplete_schema(data_type);
            };
            includes.push((semantic_argument_name(name).to_owned(), old_name.to_owned()));
        } else {
            let mut selected = Vec::new();
            if !collect_tidy_selection(&arg.value, false, false, &mut selected, &mut excludes) {
                return incomplete_schema(data_type);
            }
            includes.extend(selected.into_iter().map(|name| (name.clone(), name)));
        }
    }
    let columns = if includes.is_empty() {
        schema
            .columns
            .iter()
            .filter(|(name, _)| !excludes.contains(name))
            .cloned()
            .collect()
    } else {
        includes
            .iter()
            .filter(|(_, source)| !excludes.contains(source))
            .filter_map(|(output, source)| {
                schema
                    .columns
                    .iter()
                    .find(|(existing, _)| existing == source)
                    .map(|(_, ty)| (output.clone(), ty.clone()))
            })
            .collect()
    };
    data_type.columns = Some(Arc::new(ColumnSchema {
        columns,
        complete: schema.complete,
        locally_constructed: false,
    }));
    data_type
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
        if let Expr::Ident { name, .. } = &arg.value
            && let Some(ty) = data_type
                .columns
                .as_ref()
                .and_then(|source| source.get(name))
            && ty.columns.is_none()
        {
            result = type_with_assigned_column(result, name, ty);
        } else {
            result = incomplete_schema(result);
        }
    }
    for (name, ty) in named {
        result = if ty.mode == Mode::Null {
            drop_column(result, name)
        } else {
            type_with_assigned_column(result, name, ty.clone())
        };
    }
    result
}

fn infer_dplyr_join(arg_types: &[RType]) -> RType {
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
        for (name, ty) in &schema.columns {
            if !columns.iter().any(|(existing, _)| existing == name) {
                columns.push((name.clone(), ty.clone()));
            }
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
        scope.insert(format!("{DATA_MASK_COLUMN_PREFIX}{name}"), RType::unknown());
    }
    scope
}

fn collect_tidy_selection(
    expr: &Expr,
    excluded: bool,
    inside_combine: bool,
    includes: &mut Vec<String>,
    excludes: &mut Vec<String>,
) -> bool {
    match expr {
        Expr::Ident { name, .. } | Expr::String(name, _) => {
            if excluded {
                excludes.push(name.clone());
            } else {
                includes.push(name.clone());
            }
            true
        }
        Expr::UnaryOp {
            op: UnaryOpKind::Neg,
            expr,
            ..
        } => collect_tidy_selection(expr, true, inside_combine, includes, excludes),
        Expr::Call { func, args, .. }
            if !inside_combine
                && ident_name(func)
                    .is_some_and(|name| crate::semantic_lists::bare_name(name) == "c") =>
        {
            args.iter()
                .all(|arg| collect_tidy_selection(&arg.value, excluded, true, includes, excludes))
        }
        _ => false,
    }
}
