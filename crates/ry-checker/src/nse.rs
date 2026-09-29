use super::*;
use crate::infer::*;

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
        let sig = self.resolve_schema_sig(name)?;
        let effect = sig.schema_effect?;
        let data_index = data_mask_source_arg(&sig, args)?;
        let data_type = self.infer(&args[data_index].value, scope);
        let user_dispatch = self.resolve_user_s3_inherited_sig(name).is_some()
            || self.resolves_user_s3_dispatch(name, &data_type);
        let mut arg_types = vec![RType::unknown(); args.len()];
        arg_types[data_index] = data_type.clone();
        if matches!(effect, SchemaEffect::Join) {
            for (index, argument) in args.iter().enumerate() {
                if index != data_index {
                    arg_types[index] = self.infer(&argument.value, scope);
                }
            }
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
                    if !argument
                        .name
                        .as_deref()
                        .is_some_and(|name| is_dplyr_control_arg(semantic_argument_name(name)))
                    {
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
                let column = semantic_argument_name(raw_name);
                if mode == EvalMode::DataMask
                    && !is_dplyr_control_arg(column)
                    && !(effect == SchemaEffect::GroupBy && column == ".add")
                {
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

        let result = match effect {
            SchemaEffect::Preserve => data_type,
            SchemaEffect::AddNamedArgs => {
                schema_mutated_type(data_type, &named_results, &masked_args, args)
            }
            SchemaEffect::GroupBy => {
                let mut result = schema_mutated_type(data_type, &named_results, &masked_args, &[]);
                let has_group_spec = args.iter().enumerate().any(|(index, arg)| {
                    index != data_index
                        && !arg.name.as_deref().is_some_and(|name| {
                            matches!(semantic_argument_name(name), ".add" | ".drop")
                        })
                });
                if has_group_spec {
                    result.class = ClassVector::from_slice(&["grouped_df", "data.frame"]);
                }
                result
            }
            SchemaEffect::Transmute => {
                schema_transmuted_type(data_type, &named_results, &masked_args)
            }
            SchemaEffect::Rename => schema_renamed_type(data_type, &tidy_args, true),
            SchemaEffect::Relocate => schema_renamed_type(data_type, &tidy_args, false),
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

fn drop_column(mut data_type: RType, name: &str) -> RType {
    if let Some(schema) = &data_type.columns {
        let mut schema = (**schema).clone();
        schema.columns.retain(|(column, _)| column != name);
        data_type.columns = Some(Arc::new(schema));
    }
    data_type
}

fn masked_output_is_dynamic(args: &[&Arg]) -> bool {
    args.iter()
        .any(|arg| arg.name.is_none() && !matches!(arg.value, Expr::Ident { .. }))
}

fn schema_mutated_type(
    mut data_type: RType,
    named: &[(&str, RType)],
    masked_args: &[&Arg],
    args: &[Arg],
) -> RType {
    if let Some(keep) = args.iter().find(|arg| arg.name.as_deref() == Some(".keep")) {
        match &keep.value {
            Expr::String(value, _) if value == "all" => {}
            Expr::String(value, _) if value == "none" => {
                let complete = data_type
                    .columns
                    .as_ref()
                    .is_some_and(|schema| schema.complete)
                    && !data_type.class.contains("grouped_df");
                data_type.columns = Some(Arc::new(ColumnSchema {
                    complete,
                    ..ColumnSchema::default()
                }));
            }
            // "used" and "unused" depend on reads within expressions.
            _ => data_type = incomplete_schema(data_type),
        }
    }
    for (name, ty) in named {
        data_type = if ty.mode == Mode::Null {
            drop_column(data_type, name)
        } else {
            type_with_assigned_column(data_type, name, ty.clone())
        };
    }
    if masked_output_is_dynamic(masked_args) {
        data_type = incomplete_schema(data_type);
    }
    data_type
}

fn schema_transmuted_type(
    mut data_type: RType,
    named: &[(&str, RType)],
    masked_args: &[&Arg],
) -> RType {
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
        if ty.mode != Mode::Null {
            data_type = type_with_assigned_column(data_type, name, ty.clone());
        }
    }
    if masked_output_is_dynamic(masked_args) {
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

fn schema_renamed_type(mut data_type: RType, args: &[&Arg], require_names: bool) -> RType {
    let Some(source) = data_type.columns.as_ref() else {
        return data_type;
    };
    let mut schema = (**source).clone();
    for arg in args {
        let Some(raw_name) = arg.name.as_deref() else {
            if require_names || selected_name(&arg.value).is_none() {
                // An unsupported selector may splice named selections. For
                // relocate this can rename, even though a plain selector
                // only changes column order.
                schema.complete = false;
            }
            continue;
        };
        let Some(old_name) = selected_name(&arg.value) else {
            schema.complete = false;
            continue;
        };
        let Some((column, _)) = schema.columns.iter_mut().find(|(name, _)| name == old_name) else {
            schema.complete = false;
            continue;
        };
        *column = semantic_argument_name(raw_name).to_owned();
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
            if !collect_tidy_selection(&arg.value, false, &mut selected, &mut excludes) {
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
        complete: !data_type.class.contains("grouped_df") && !masked_output_is_dynamic(masked_args),
        ..ColumnSchema::default()
    };
    if let Some(by) = args.iter().find(|arg| arg.name.as_deref() == Some(".by")) {
        let mut includes = Vec::new();
        let mut excludes = Vec::new();
        if !collect_tidy_selection(&by.value, false, &mut includes, &mut excludes) {
            schema.complete = false;
        } else if let Some(source) = &data_type.columns {
            let selected: Vec<_> = if includes.is_empty() {
                source
                    .columns
                    .iter()
                    .filter(|(name, _)| !excludes.contains(name))
                    .cloned()
                    .collect()
            } else {
                includes
                    .iter()
                    .filter(|name| !excludes.contains(name))
                    .filter_map(|name| source.columns.iter().find(|(column, _)| column == name))
                    .cloned()
                    .collect()
            };
            schema.columns.extend(selected);
            schema.complete &= source.complete;
        } else {
            schema.complete = false;
        }
    }
    let mut result = RType::new(Mode::List, Length::One)
        .with_class(ClassVector::single("data.frame"))
        .with_columns(Arc::new(schema));
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
        } => collect_tidy_selection(expr, true, includes, excludes),
        Expr::Call { func, args, .. }
            if ident_name(func)
                .is_some_and(|name| crate::semantic_lists::bare_name(name) == "c") =>
        {
            args.iter()
                .all(|arg| collect_tidy_selection(&arg.value, excluded, includes, excludes))
        }
        _ => false,
    }
}
