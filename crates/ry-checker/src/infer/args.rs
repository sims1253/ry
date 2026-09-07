//! R argument matching, evaluation modes, and call diagnostics.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ArgumentMatch {
    /// Formal parameter index for each actual argument. `None` means the
    /// argument was unmatched (or was absorbed by `...`).
    pub(crate) param_for_arg: Vec<Option<usize>>,
    pub(crate) bound_params: Vec<bool>,
    pub(crate) unmatched_named: Vec<usize>,
    pub(crate) dots: Option<usize>,
}

impl ArgumentMatch {
    /// The actual argument bound to formal parameter `formal_index`,
    /// if any: the reverse of `param_for_arg`. Repeated names
    /// (`f(x = 1, x = 2)`, a runtime error in R) bind the first actual.
    pub(crate) fn arg_for_param(&self, formal_index: usize) -> Option<usize> {
        self.param_for_arg
            .iter()
            .position(|bound| *bound == Some(formal_index))
    }
}

/// Match R call arguments in the same three passes as `match.call`: exact
/// names, unambiguous partial names, then unnamed arguments positionally.
/// Partial and positional matching stop at `...`; exact names may still bind
/// formals declared after it.
pub(crate) fn match_arguments(param_names: &[&str], args: &[Arg]) -> ArgumentMatch {
    match_argument_names(
        param_names,
        args.iter().map(|argument| argument.name.as_deref()),
    )
}

pub(crate) fn match_argument_names<'a>(
    param_names: &[&str],
    names: impl Iterator<Item = Option<&'a str>> + Clone,
) -> ArgumentMatch {
    let dots = param_names.iter().position(|name| *name == "...");
    let partial_end = dots.unwrap_or(param_names.len());
    let mut result = ArgumentMatch {
        param_for_arg: vec![None; names.clone().count()],
        bound_params: vec![false; param_names.len()],
        unmatched_named: Vec::new(),
        dots,
    };

    // Pass 1: exact names match every formal, including formals after `...`.
    for (argument_index, name) in names.clone().enumerate() {
        let Some(name) = name else {
            continue;
        };
        if let Some(parameter_index) = param_names
            .iter()
            .position(|parameter| *parameter != "..." && *parameter == name)
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 2: only a unique prefix of a pre-dots formal is a partial match.
    for (argument_index, name) in names.clone().enumerate() {
        if result.param_for_arg[argument_index].is_some() {
            continue;
        }
        let Some(name) = name else {
            continue;
        };
        let mut candidates =
            param_names[..partial_end]
                .iter()
                .enumerate()
                .filter(|(index, parameter)| {
                    !result.bound_params[*index] && parameter.starts_with(name)
                });
        let first = candidates.next().map(|(index, _)| index);
        if let Some(parameter_index) = first
            && candidates.next().is_none()
        {
            result.param_for_arg[argument_index] = Some(parameter_index);
            result.bound_params[parameter_index] = true;
        }
    }

    // Pass 3: unnamed actuals fill the remaining pre-dots formals in order.
    let mut next_parameter = 0;
    for (argument_index, name) in names.clone().enumerate() {
        if name.is_some() {
            if result.param_for_arg[argument_index].is_none() {
                result.unmatched_named.push(argument_index);
            }
            continue;
        }
        while next_parameter < partial_end && result.bound_params[next_parameter] {
            next_parameter += 1;
        }
        if next_parameter < partial_end {
            result.param_for_arg[argument_index] = Some(next_parameter);
            result.bound_params[next_parameter] = true;
            next_parameter += 1;
        }
    }
    result
}

/// The formal-parameter view RY090/RY091 reporting needs, shared by
/// typeshed `ParamSpec`s and collected `UserParam`s so one code path
/// serves stub and user calls.
trait CallFormal {
    fn name(&self) -> &str;
    fn required(&self) -> bool;
}

impl CallFormal for ParamSpec {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

impl CallFormal for UserParam {
    fn name(&self) -> &str {
        &self.name
    }
    fn required(&self) -> bool {
        self.required
    }
}

/// `match_arguments` over a signature's formal specs: collect the formal
/// names once and run R's three-pass matching. Callers that need the
/// names themselves (message text, eval-mode lookup) keep their own
/// `param_names()` vector.
pub(crate) fn match_params(params: &[ParamSpec], args: &[Arg]) -> ArgumentMatch {
    let names: Vec<&str> = params.iter().map(|param| param.name.as_str()).collect();
    match_arguments(&names, args)
}

/// Return the actual argument bound to a formal under ordinary R matching.
/// Semantic metadata must use this rather than raw call positions.
pub(crate) fn bound_argument_index(
    params: &[ParamSpec],
    args: &[Arg],
    formal: &str,
) -> Option<usize> {
    bound_argument_index_matched(params, &match_params(params, args), formal)
}

/// `bound_argument_index` over an argument match already computed for
/// this call, so a site that consults several formals matches once.
pub(crate) fn bound_argument_index_matched(
    params: &[ParamSpec],
    bindings: &ArgumentMatch,
    formal: &str,
) -> Option<usize> {
    bindings.arg_for_param(params.iter().position(|param| param.name == formal)?)
}

pub(crate) fn match_args_to_params(
    sig_params: &[ParamSpec],
    args: &[Arg],
    arg_types: &[RType],
) -> Vec<RType> {
    let bindings = match_params(sig_params, args);
    let mut matched = vec![RType::unknown(); sig_params.len()];
    for (formal_index, slot) in matched.iter_mut().enumerate() {
        if let Some(argument_type) = bindings
            .arg_for_param(formal_index)
            .and_then(|argument_index| arg_types.get(argument_index))
        {
            *slot = argument_type.clone();
        }
    }
    matched
}

impl Checker {
    fn is_forwarded_dots(&self, argument: &Arg) -> bool {
        let Expr::Ident { name, span } = &argument.value else {
            return false;
        };
        // Lowering removes parentheses. Only the original direct symbol is
        // expanded by R; `f((...))` passes an ordinary expression instead.
        semantic_argument_name(name) == "..."
            && argument.span.end == span.end
            && self.source.get(span.start..span.end) == Some(name.as_str())
    }

    /// Non-firing policy for schema calls:
    /// - RY090 stays silent for `...`, successful exact/partial matches, and
    ///   legacy inference-only signatures without completeness metadata.
    /// - RY091 stays silent for every non-required or successfully bound
    ///   parameter.
    /// - RY092 stays silent without a declared type, for opaque/unknown
    ///   actuals, whenever a union has any compatible overlap, and for R's
    ///   logical/integer/double coercion family.
    pub(crate) fn check_typeshed_call_arguments(
        &mut self,
        function_name: &str,
        signature: &FunctionSig,
        args: &[Arg],
        arg_types: &[RType],
        call_span: Span,
    ) {
        let bindings = match_params(&signature.params, args);
        // `...` accepts every otherwise-unmatched actual argument. Without
        // it, report only named arguments; excess positionals are outside
        // this rule's deliberately narrow scope.
        let supports_unknown_argument_check = signature
            .params
            .iter()
            .any(|param| param.required || param.default.is_some() || param.type_.is_some());
        self.check_call_arity(
            function_name,
            &signature.params,
            args,
            &bindings,
            supports_unknown_argument_check,
            call_span,
        );

        for (argument_index, parameter_index) in bindings.param_for_arg.iter().enumerate() {
            let Some(parameter_index) = parameter_index else {
                continue;
            };
            let parameter = &signature.params[*parameter_index];
            let Some(expected_json) = parameter.type_.as_ref() else {
                continue;
            };
            let expected = json_rtype_to_rtype(expected_json);
            let Some(actual) = arg_types.get(argument_index) else {
                continue;
            };
            if generic_argument_may_dispatch(&self.typeshed.globals, function_name, actual) {
                continue;
            }
            if types_provably_incompatible(actual, &expected) {
                self.emit(
                    Severity::Error,
                    args[argument_index].span,
                    "RY092",
                    format!(
                        "argument `{}` to `{function_name}` is `{}`, expected {}",
                        parameter.name,
                        actual.mode,
                        expected_type_label(&expected)
                    ),
                );
            }
        }
    }

    pub(crate) fn check_user_call_arguments(
        &mut self,
        function_name: &str,
        function: &UserFn,
        args: &[Arg],
        call_span: Span,
    ) {
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let bindings = match_arguments(&names, args);
        self.check_call_arity(
            function_name,
            &function.params,
            args,
            &bindings,
            true,
            call_span,
        );
    }

    /// Shared arity reporting over one argument match: RY090 for named
    /// actuals no formal matched, RY091 for required formals without a
    /// supplied value. The typeshed and user-function checks differ only in their
    /// formals source and unknown-argument gating.
    fn check_call_arity<P: CallFormal>(
        &mut self,
        function_name: &str,
        params: &[P],
        args: &[Arg],
        bindings: &ArgumentMatch,
        report_unknown: bool,
        call_span: Span,
    ) {
        let names: Vec<&str> = params.iter().map(|param| param.name()).collect();
        let required: Vec<bool> = params.iter().map(|param| param.required()).collect();
        self.emit_unknown_arguments(function_name, &names, args, bindings, report_unknown);
        self.emit_missing_required(function_name, &names, &required, args, bindings, call_span);
    }

    fn emit_unknown_arguments(
        &mut self,
        function_name: &str,
        names: &[&str],
        args: &[Arg],
        bindings: &ArgumentMatch,
        enabled: bool,
    ) {
        if !enabled || bindings.dots.is_some() {
            return;
        }
        let forwards_dots = args.iter().any(|argument| self.is_forwarded_dots(argument));
        for argument_index in &bindings.unmatched_named {
            let argument = &args[*argument_index];
            // R expands the dots actuals and ignores the tag on the dots expression.
            if self.is_forwarded_dots(argument) {
                continue;
            }
            let argument_name = argument.name.as_deref().unwrap_or_default();
            // Expanded exact names can change which partial matches are valid.
            if forwards_dots
                && names.iter().any(|name| {
                    semantic_argument_name(name).starts_with(semantic_argument_name(argument_name))
                })
            {
                continue;
            }
            let suggestion = closest_parameter(argument_name, names);
            let hint = suggestion
                .map(|name| format!("; did you mean `{name}`?"))
                .unwrap_or_default();
            let message = format!("unknown argument `{argument_name}` to `{function_name}`{hint}");
            self.emit(Severity::Warning, argument.span, "RY090", message);
        }
    }

    fn emit_missing_required(
        &mut self,
        function_name: &str,
        names: &[&str],
        required: &[bool],
        args: &[Arg],
        bindings: &ArgumentMatch,
        call_span: Span,
    ) {
        let forwards_dots = args.iter().any(|argument| self.is_forwarded_dots(argument));
        for (parameter_index, required) in required.iter().enumerate() {
            // Forwarded actuals can fill unmatched slots or change positional and
            // partial matches. Only an exact named hole pins a missing formal.
            let missing = if forwards_dots {
                args.iter().any(|argument| {
                    argument.name.as_deref().is_some_and(|name| {
                        semantic_argument_name(name)
                            == semantic_argument_name(names[parameter_index])
                    }) && matches!(argument.value, Expr::Missing(_))
                })
            } else {
                !bindings.bound_params[parameter_index]
                    || bindings
                        .arg_for_param(parameter_index)
                        .is_some_and(|index| matches!(args[index].value, Expr::Missing(_)))
            };
            if *required && missing {
                self.emit(
                    Severity::Warning,
                    call_span,
                    "RY091",
                    format!(
                        "missing required argument `{}` in call to `{function_name}`",
                        names[parameter_index]
                    ),
                );
            }
        }
    }
}

fn closest_parameter<'a>(argument: &str, parameters: &'a [&str]) -> Option<&'a str> {
    let mut closest = None;
    let mut minimum_distance = usize::MAX;
    let mut minimum_is_tied = false;

    for parameter in parameters.iter().copied().filter(|name| *name != "...") {
        let distance = edit_distance(argument, parameter);
        if distance > 2 {
            continue;
        }
        match distance.cmp(&minimum_distance) {
            std::cmp::Ordering::Less => {
                closest = Some(parameter);
                minimum_distance = distance;
                minimum_is_tied = false;
            }
            std::cmp::Ordering::Equal => minimum_is_tied = true,
            std::cmp::Ordering::Greater => {}
        }
    }

    if minimum_is_tied { None } else { closest }
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right_chars.len() + 1);
        current.push(left_index + 1);
        for (right_index, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_char != *right_char);
            current.push(
                (current[right_index] + 1)
                    .min(previous[right_index + 1] + 1)
                    .min(substitution),
            );
        }
        previous = current;
    }
    previous[right_chars.len()]
}

/// Whether RY092 should defer its argument mode check: a classed or NULL
/// actual may dispatch to a method that accepts it.
fn generic_argument_may_dispatch(
    globals: &ry_typeshed::Globals,
    function_name: &str,
    actual: &RType,
) -> bool {
    let generic = crate::higher_order::is_dispatch_capable_generic(globals, function_name);
    generic && (actual.class.has_known_class() || actual.mode == Mode::Null)
}

/// Eval mode declared for the argument at `index`.
///
/// `bindings` comes from one `match_params` call shared by the whole call
/// site, so a loop over arguments does not re-match per argument.
pub(crate) fn eval_mode_for_arg(
    sig: &FunctionSig,
    bindings: &ArgumentMatch,
    index: usize,
) -> Option<EvalMode> {
    let parameter = bindings
        .param_for_arg
        .get(index)?
        .and_then(|parameter_index| sig.params.get(parameter_index))
        .map(|param| param.name.as_str())
        .unwrap_or("...");
    sig.eval
        .get(parameter)
        .copied()
        .or_else(|| sig.eval.get("...").copied())
}

pub(crate) fn argument_eval_mode(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<EvalMode> {
    eval_mode_for_arg(sig, &match_params(&sig.params, args), index)
}

/// Locate the supplied argument named by a signature's data-mask source.
/// Formula APIs place `data` after their quoted formula, and some calls put it
/// after mask-evaluated arguments, so callers must not assume argument zero.
pub(crate) fn data_mask_source_arg(sig: &FunctionSig, args: &[Arg]) -> Option<usize> {
    let source = sig.data_mask_source.as_deref()?;
    let bindings = match_params(&sig.params, args);
    bound_argument_index_matched(&sig.params, &bindings, source)
}

/// Whether the matched formal processes tidy-evaluation injection syntax.
pub(crate) fn argument_supports_injection(
    sig: &FunctionSig,
    args: &[Arg],
    index: usize,
) -> Option<InjectionMode> {
    let bindings = match_params(&sig.params, args);
    bindings.param_for_arg[index]
        .or(bindings.dots)
        .and_then(|formal| sig.params.get(formal))
        .and_then(|param| sig.injection.get(&param.name).copied())
}

impl Checker {
    pub(crate) fn infer_with_injection(
        &mut self,
        expr: &Expr,
        scope: &mut Scope,
        enabled: Option<InjectionMode>,
    ) -> RType {
        let previous = scope.tidy_injection;
        scope.tidy_injection = enabled.max(previous);
        let result = self.infer(expr, scope);
        scope.tidy_injection = previous;
        result
    }
}

#[cfg(test)]
mod argument_matching_tests {
    use super::*;

    fn argument(name: Option<&str>) -> Arg {
        Arg {
            name: name.map(str::to_string),
            value: Expr::Null(Span::default()),
            span: Span::default(),
        }
    }

    #[test]
    fn exact_names_are_matched_before_positionals() {
        let args = [argument(Some("second")), argument(None)];
        let matched = match_arguments(&["first", "second"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(1), Some(0)]);
        assert_eq!(matched.bound_params, vec![true, true]);
    }

    #[test]
    fn exact_match_is_removed_before_partial_matching() {
        let args = [argument(Some("alpha")), argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), Some(1)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn unique_partial_name_matches() {
        let args = [argument(Some("alp"))];
        let matched = match_arguments(&["alpha", "beta"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0)]);
        assert!(matched.unmatched_named.is_empty());
    }

    #[test]
    fn ambiguous_partial_name_stays_unmatched() {
        let args = [argument(Some("al"))];
        let matched = match_arguments(&["alpha", "alpine"], &args);
        assert_eq!(matched.param_for_arg, vec![None]);
        assert_eq!(matched.unmatched_named, vec![0]);
    }

    #[test]
    fn dots_absorb_remaining_arguments_and_stop_positionals() {
        let args = [argument(None), argument(None), argument(Some("extra"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(0), None, None]);
        assert_eq!(matched.unmatched_named, vec![2]);
        assert_eq!(matched.dots, Some(1));
    }

    #[test]
    fn formal_lookup_supports_positional_exact_and_partial_matching() {
        let params = ["file", "local", "..."]
            .into_iter()
            .map(|name| ParamSpec {
                name: name.to_string(),
                type_: None,
                required: false,
                default: None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            bound_argument_index(&params, &[argument(None), argument(None)], "local"),
            Some(1)
        );
        assert_eq!(
            bound_argument_index(
                &params,
                &[argument(Some("local")), argument(Some("file"))],
                "local"
            ),
            Some(0)
        );
        assert_eq!(
            bound_argument_index(&params, &[argument(Some("lo"))], "local"),
            Some(0)
        );
    }

    #[test]
    fn exact_name_after_dots_still_matches_but_partial_does_not() {
        let args = [argument(Some("after")), argument(Some("aft"))];
        let matched = match_arguments(&["x", "...", "after"], &args);
        assert_eq!(matched.param_for_arg, vec![Some(2), None]);
    }

    #[test]
    fn opaque_union_member_keeps_type_check_silent() {
        let actual = RType::union(Arc::from(vec![
            RType::unknown(),
            RType::scalar(Mode::Character),
        ]));
        let expected = RType::scalar(Mode::Double);
        assert!(!types_provably_incompatible(&actual, &expected));
    }

    #[test]
    fn closest_parameter_is_limited_to_edit_distance_two() {
        assert_eq!(
            closest_parameter("lenght", &["length", "x"]),
            Some("length")
        );
        assert_eq!(closest_parameter("unrelated", &["length", "x"]), None);
    }
}
