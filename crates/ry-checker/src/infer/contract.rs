//! Call contracts: what a call reveals through a project definition whose
//! whole body is one bare formal or one base operation on it (#646).
//!
//! A contract is derived from the literal's syntax alone and travels with
//! its definition (`UserFn::contract`), so a call uses it only through the
//! selected call head, never through a fresh name lookup. The body shapes
//! are deliberately tiny: R evaluates braces, parentheses, `return`, and
//! every operator through ordinary bindings, so any wider shape needs
//! proof that each of them is base (a masked `` `{` `` or `` `return` ``
//! makes `function(x) { x }` or `function(x) return(x)` return 10).

use super::callee::CallHead;
use super::*;

/// The fact a call to one definition carries for the actual bound to `formal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallContract {
    /// The body is exactly the formal: the call returns its actual.
    Identity { formal: usize },
    /// The body is one base operation on the formal and literals, which
    /// errors unless the formal's value can take part.
    Demand {
        formal: usize,
        demand: Demand,
        /// The body's arithmetic operator, also for `df$value * 2`.
        arithmetic: Option<BinOpKind>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Demand {
    /// An arithmetic operand beside a numeric or logical literal.
    ArithmeticOperand,
    /// The receiver of `$`.
    DollarReceiver,
}

impl CallContract {
    /// The contract of `function(params) body` spanning `span`. Its body
    /// reads no other formal, so no other default is ever forced. A call
    /// uses the contract only when it supplies the formal, so the formal's
    /// own default never runs either; refusing a default that may write the
    /// formal is belt and braces.
    pub(crate) fn derive(params: &[Param], body: &[Stmt], span: Span) -> Option<Self> {
        let [Stmt::Expr(expr)] = body else {
            return None;
        };
        // Lowering drops braces and parentheses but keeps the inner span,
        // so a wrapped body or operand ends (or starts) inside its parent.
        if span_of(expr).end != span.end {
            return None;
        }
        let formal = |expr: &Expr| match expr {
            Expr::Ident { name, .. } if semantic_argument_name(name) != "..." => {
                params.iter().position(|param| {
                    semantic_argument_name(&param.name) == semantic_argument_name(name)
                })
            }
            _ => None,
        };
        let receiver = |expr: &Expr| match expr {
            Expr::Index {
                base,
                kind: IndexKind::Dollar,
                span,
                ..
            } if span_of(base).start == span.start => formal(base),
            _ => None,
        };
        let operand = |expr: &Expr| {
            formal(expr)
                .map(|formal| (formal, Demand::ArithmeticOperand))
                .or_else(|| receiver(expr).map(|formal| (formal, Demand::DollarReceiver)))
        };
        let contract = if let Some(formal) = formal(expr) {
            Self::Identity { formal }
        } else if let Some(formal) = receiver(expr) {
            Self::Demand {
                formal,
                demand: Demand::DollarReceiver,
                arithmetic: None,
            }
        } else {
            let Expr::BinOp { op, lhs, rhs, span } = expr else {
                return None;
            };
            if !op.is_arithmetic()
                || span_of(lhs).start != span.start
                || span_of(rhs).end != span.end
            {
                return None;
            }
            let literal = |expr: &Expr| {
                matches!(
                    expr,
                    Expr::Integer(..) | Expr::Double(..) | Expr::Logical(..)
                )
            };
            let (formal, demand) = match (operand(lhs), operand(rhs)) {
                (Some(demanded), None) if literal(rhs) => demanded,
                (None, Some(demanded)) if literal(lhs) => demanded,
                _ => return None,
            };
            Self::Demand {
                formal,
                demand,
                arithmetic: Some(*op),
            }
        };
        let formal_masks = contract.operators().any(|operator| {
            params
                .iter()
                .any(|param| semantic_argument_name(&param.name) == operator)
        });
        let name = semantic_argument_name(&params[contract.formal()].name);
        (!formal_masks && !crate::collect::default_local_writes(params).may_write(name))
            .then_some(contract)
    }

    pub(crate) fn formal(self) -> usize {
        match self {
            Self::Identity { formal } | Self::Demand { formal, .. } => formal,
        }
    }

    /// Operators the body looks up in the definition's environment.
    pub(crate) fn operators(self) -> impl Iterator<Item = &'static str> {
        let (receiver, arithmetic) = match self {
            Self::Identity { .. } => (false, None),
            Self::Demand {
                demand, arithmetic, ..
            } => (demand == Demand::DollarReceiver, arithmetic),
        };
        receiver
            .then_some("$")
            .into_iter()
            .chain(arithmetic.map(op_symbol))
    }
}

impl Checker {
    /// The selected definition's contract and the index of the actual bound
    /// to its formal. Every operator the body evaluates must be base in the
    /// definition's environment: no project, imported, or escaped binding
    /// masks it (a global `` `+` `` makes `x + 1` paste). Methods need no
    /// screen: R dispatches operators only on objects, and a demand is
    /// reported only for an unclassed actual beside a literal.
    fn contract_actual<'h>(
        &self,
        head: &'h CallHead,
        args: &[Arg],
    ) -> Option<(&'h UserFn, CallContract, usize)> {
        let (function, contract) = head.contract()?;
        let unscoped = Scope::default();
        if contract.operators().any(|operator| {
            self.has_explicit_operator_mask(operator, &format!("`{operator}`"), &unscoped)
        }) || args
            .iter()
            .any(|arg| matches!(&arg.value, Expr::Ident { name, .. } if name == "..."))
        {
            return None;
        }
        let parameter = &function.params[contract.formal()];
        if parameter.quoting || parameter.defused {
            return None;
        }
        let names: Vec<&str> = function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        let index = match_arguments(&names, args).arg_for_param(contract.formal())?;
        // R forces only the bound actual, but every actual was inferred in
        // the caller's scope, so an effect in another one (`f({v <- "a"},
        // x = v)`) may have changed the bound actual's type although it
        // never runs. Other actuals must be inert: a literal or a bare name.
        let inert = |value: &Expr| {
            matches!(
                value,
                Expr::Logical(..)
                    | Expr::Integer(..)
                    | Expr::Double(..)
                    | Expr::String(..)
                    | Expr::Null(_)
                    | Expr::Na(..)
                    | Expr::Ident { .. }
                    | Expr::Missing(_)
            )
        };
        let others_inert = args
            .iter()
            .enumerate()
            .all(|(position, arg)| position == index || inert(&arg.value));
        (others_inert && !matches!(args[index].value, Expr::Missing(_)))
            .then_some((function, contract, index))
    }

    /// RY092 for an actual the selected definition's body cannot evaluate.
    pub(super) fn check_contract_demand(
        &mut self,
        function_name: &str,
        head: &CallHead,
        args: &[Arg],
        arg_types: &[RType],
    ) {
        let Some((
            function,
            CallContract::Demand {
                formal,
                demand,
                arithmetic,
            },
            index,
        )) = self.contract_actual(head, args)
        else {
            return;
        };
        let Some(actual) = arg_types.get(index) else {
            return;
        };
        let parameter = &function.params[formal].name;
        let use_ = match (demand, arithmetic) {
            (Demand::ArithmeticOperand, Some(op))
                if !actual.class.is_unknown()
                    && !actual.class.has_known_class()
                    && matches!(actual.mode, Mode::Character | Mode::List | Mode::Function) =>
            {
                format!("an operand of `{}`", op_symbol(op))
            }
            (Demand::DollarReceiver, _)
                if dollar_receiver_is_definitely_atomic(actual)
                    && !dollar_atomic_receiver_may_dispatch(actual) =>
            {
                "the receiver of `$`, which is invalid for atomic vectors".to_string()
            }
            _ => return,
        };
        self.emit(
            Severity::Error,
            args[index].span,
            "RY092",
            format!(
                "argument `{parameter}` to `{function_name}` is `{}`, but its body uses it as {use_}",
                dollar_receiver_mode_description(actual)
            ),
        );
    }

    /// The actual's type, when the selected definition returns it unchanged.
    pub(super) fn contract_return(
        &self,
        head: &CallHead,
        args: &[Arg],
        arg_types: &[RType],
    ) -> Option<RType> {
        match self.contract_actual(head, args)? {
            (_, CallContract::Identity { .. }, index) => arg_types.get(index).cloned(),
            _ => None,
        }
    }
}
