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
        span: Span,
    ) -> Option<RType> {
        let sig = self.resolve_schema_sig(name)?;
        let effect = sig.schema_effect?;
        let first = args.first()?;
        let data_type = self.infer(&first.value, scope);
        let user_dispatch = self.resolve_user_s3_inherited_sig(name).is_some()
            || self.resolves_user_s3_dispatch(name, &data_type);
        let mut arg_types = Vec::with_capacity(args.len());
        arg_types.push(data_type.clone());
        if matches!(effect, SchemaEffect::Join) {
            arg_types.extend(
                args.iter()
                    .skip(1)
                    .map(|argument| self.infer(&argument.value, scope)),
            );
            return Some(self.infer_dplyr_join(&arg_types));
        }

        let mut local = self.dplyr_data_mask_scope(scope, &data_type);
        if user_dispatch {
            local = local.with_unknown_data_mask();
        }
        let mut named_results = Vec::new();
        let mut tidy_args = Vec::new();
        for (index, argument) in args.iter().enumerate().skip(1) {
            let mode = argument_eval_mode(&sig, args, index).unwrap_or(EvalMode::Normal);
            let inferred = match mode {
                EvalMode::Normal => self.infer(&argument.value, scope),
                EvalMode::DataMask => {
                    local.insert(".", RType::unknown());
                    self.infer(&argument.value, &mut local)
                }
                EvalMode::TidySelect => {
                    tidy_args.push(&argument.value);
                    self.infer_tidyselect_expr(&argument.value, &mut local)
                }
                EvalMode::QuotedSymbol => {
                    if matches!(argument.value, Expr::Ident { .. }) {
                        RType::unknown()
                    } else {
                        self.infer(&argument.value, &mut local)
                    }
                }
                EvalMode::QuotedExpression => RType::unknown(),
            };
            if let Some(raw_name) = argument.name.as_deref() {
                let column = semantic_argument_name(raw_name);
                if !is_dplyr_control_arg(&column) {
                    local.insert(column.clone(), inferred.clone());
                    local.insert(
                        format!("{DATA_MASK_COLUMN_PREFIX}{column}"),
                        RType::unknown(),
                    );
                    named_results.push((column, inferred.clone()));
                }
            }
            arg_types.push(inferred);
        }

        let result = match effect {
            SchemaEffect::Preserve => data_type,
            SchemaEffect::AddNamedArgs => named_results
                .into_iter()
                .fold(data_type, |result, (name, ty)| {
                    type_with_assigned_column(result, &name, ty)
                }),
            SchemaEffect::Select => self.schema_selected_type(data_type, &tidy_args),
            SchemaEffect::Aggregate => {
                let mut result = RType::new(Mode::List, Length::One)
                    .with_class(ClassVector::single("data.frame"));
                for (name, ty) in named_results {
                    result = type_with_assigned_column(result, &name, ty);
                }
                result
            }
            SchemaEffect::ExpressionValue => {
                arg_types.get(1).cloned().unwrap_or_else(RType::unknown)
            }
            SchemaEffect::Join => unreachable!("joins return before data-mask evaluation"),
            SchemaEffect::Pivot => RType::new(Mode::List, Length::Unknown)
                .with_class(ClassVector::single("data.frame")),
        };
        let _ = span;
        Some(result)
    }

    fn schema_selected_type(&self, mut data_type: RType, args: &[&Expr]) -> RType {
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
        }));
        data_type
    }

    pub(crate) fn infer_dplyr_join(&self, arg_types: &[RType]) -> RType {
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
            result = result.with_columns(Arc::new(ColumnSchema { columns, complete }));
        }
        result
    }

    pub(crate) fn infer_tidyselect_expr(&mut self, expr: &Expr, scope: &mut Scope) -> RType {
        match expr {
            Expr::String(_, _) => RType::scalar(Mode::Character),
            Expr::Ident { name, .. } => scope.get(name).cloned().unwrap_or_else(RType::unknown),
            Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            } => {
                let _ = self.infer_tidyselect_expr(expr, scope);
                RType::unknown()
            }
            Expr::Call { func, args, .. }
                if ident_name(func).is_some_and(|name| {
                    name.rsplit_once("::").map(|(_, n)| n).unwrap_or(name) == "c"
                }) =>
            {
                for a in args {
                    let _ = self.infer_tidyselect_expr(&a.value, scope);
                }
                RType::unknown()
            }
            _ => self.infer(expr, scope),
        }
    }

    pub(crate) fn scope_with_columns(
        &self,
        base_scope: &Scope,
        schema: &Arc<ColumnSchema>,
    ) -> Scope {
        let mut scope = base_scope.clone();
        for (name, ty) in &schema.columns {
            scope.insert(name.clone(), ty.clone());
            scope.insert(format!("{DATA_MASK_COLUMN_PREFIX}{name}"), RType::unknown());
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
            Some(schema) => self.scope_with_columns(base_scope, schema),
            None => base_scope.clone(),
        };
        scope.insert(DATA_MASK_ACTIVE, RType::unknown());
        scope.insert(".data", df_type.clone());
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
            if ident_name(func).is_some_and(|name| {
                name.rsplit_once("::").map(|(_, bare)| bare).unwrap_or(name) == "c"
            }) =>
        {
            args.iter()
                .all(|arg| collect_tidy_selection(&arg.value, excluded, includes, excludes))
        }
        _ => false,
    }
}
