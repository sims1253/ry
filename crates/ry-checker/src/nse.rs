use super::*;
use crate::infer::*;

pub(crate) const DATA_MASK_ACTIVE: &str = "\0ry_data_mask";
pub(crate) const DATA_MASK_ENV_PREFIX: &str = "\0ry_data_mask_env:";
pub(crate) const DATA_MASK_COLUMN_PREFIX: &str = "\0ry_data_mask_column:";

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
                    local.insert(".", RType::unknown());
                    self.infer_with_injection(&argument.value, &mut local, injection)
                }
                EvalMode::TidySelect => {
                    tidy_args.push(&argument.value);
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
                if !is_dplyr_control_arg(column) {
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
            SchemaEffect::AddNamedArgs => named_results
                .into_iter()
                .fold(data_type, |result, (name, ty)| {
                    type_with_assigned_column(result, name, ty)
                }),
            SchemaEffect::Select => schema_selected_type(data_type, &tidy_args),
            SchemaEffect::Aggregate => {
                let mut result = RType::new(Mode::List, Length::One)
                    .with_class(ClassVector::single("data.frame"));
                for (name, ty) in named_results {
                    result = type_with_assigned_column(result, name, ty);
                }
                result
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

fn schema_selected_type(mut data_type: RType, args: &[&Expr]) -> RType {
    if args.is_empty() {
        return data_type;
    }
    let Some(schema) = data_type.columns.as_ref() else {
        return data_type;
    };
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    for expr in args {
        if !collect_tidy_selection(expr, false, &mut includes, &mut excludes) {
            return data_type;
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
            .filter(|name| !excludes.contains(name))
            .filter_map(|name| {
                schema
                    .columns
                    .iter()
                    .find(|(existing, _)| existing == name)
                    .cloned()
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
