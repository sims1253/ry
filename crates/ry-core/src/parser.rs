//! Tree-sitter-r adapter. Parses raw R source into our `ast::SourceFile`.
//!
//! Strategy: walk the tree-sitter CST and lower into our AST. Anything
//! unrecognized becomes `Expr::Unknown(span)`, never panics.

use crate::ast::*;
use crate::span::Span;
use thiserror::Error;
use tree_sitter::{Node, Parser};

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("tree-sitter parse error at {line}:{col}: {message}")]
    TreeSitter {
        line: usize,
        col: usize,
        message: String,
    },
}

pub struct RParser {
    parser: Parser,
}

impl RParser {
    pub fn new() -> Result<Self, ParseError> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_r::LANGUAGE.into())
            .map_err(|e| ParseError::TreeSitter {
                line: 0,
                col: 0,
                message: format!("language load failed: {}", e),
            })?;
        Ok(Self { parser })
    }

    pub fn parse(&mut self, path: &str, src: &str) -> Result<SourceFile, ParseError> {
        let tree = self
            .parser
            .parse(src, None)
            .ok_or_else(|| ParseError::TreeSitter {
                line: 0,
                col: 0,
                message: "parser returned no tree".into(),
            })?;
        let root = tree.root_node();
        let mut stmts = Vec::new();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            // A top-level `{ a <- 1; b <- 2 }` braced expression must
            // splice ALL of its child statements into the surrounding
            // statement list, not just the last one. The earlier
            // `lower_braced_as_stmt` discarded earlier siblings.
            if child.kind() == "braced_expression" {
                let mut inner_cursor = child.walk();
                for inner in child.named_children(&mut inner_cursor) {
                    if let Some(stmt) = self.lower_stmt(inner, src) {
                        stmts.push(stmt);
                    }
                }
                continue;
            }
            if let Some(stmt) = self.lower_stmt(child, src) {
                stmts.push(stmt);
            }
        }
        // tree-sitter always returns a tree, even for broken input; it
        // marks unrecoverable regions with `ERROR` nodes and missing
        // tokens with `MISSING` nodes. Collect these so the checker can
        // surface them as RY000 instead of silently checking a recovered
        // (and possibly nonsense) tree.
        let parse_errors = collect_parse_errors(root);
        Ok(SourceFile {
            path: path.to_string(),
            stmts,
            parse_errors,
        })
    }

    fn span(&self, n: Node, _src: &str) -> Span {
        let start = n.start_byte();
        let end = n.end_byte();
        let pos = n.start_position();
        // tree-sitter reports both row and column for free; the previous
        // implementation discarded `.column` and recomputed a *char*
        // column by rescanning the whole file from byte 0 for every node
        // (O(n^2) total). We now use the byte column tree-sitter gives us
        // directly. `Span::col` is therefore byte-indexed within the line;
        // diagnostics rendering that need a char column convert per-line.
        Span::new(start, end, pos.row, pos.column)
    }

    fn lower_stmt(&self, n: Node, src: &str) -> Option<Stmt> {
        match n.kind() {
            "binary_operator" => {
                if let Some(b) = self.try_lower_assign(n, src) {
                    return Some(b);
                }
                Some(Stmt::Expr(self.lower_binary(n, src)?))
            }
            "call" => Some(Stmt::Expr(self.lower_call(n, src)?)),
            "identifier" => Some(Stmt::Expr(Expr::Ident {
                name: text(n, src)?,
                span: self.span(n, src),
            })),
            "if_statement" => self.lower_if(n, src),
            "for_statement" => self.lower_for(n, src),
            "while_statement" => Some(self.lower_while(n, src, false)),
            "repeat_statement" => Some(self.lower_repeat(n, src)),
            "braced_expression" => self.lower_braced_as_stmt(n, src),
            "function_definition" => self.lower_function_def_as_stmt(n, src),
            _ => {
                // Fallback: any expression node (string, integer, float,
                // na, null, true, false, subset, etc.) appearing in
                // statement position is wrapped as a bare expression
                // statement. Without this, function-body trailing
                // expressions like `function() "hello"` would be silently
                // dropped, breaking return-type inference.
                if let Some(e) = self.lower_expr(n, src) {
                    Some(Stmt::Expr(e))
                } else {
                    tracing::trace!(kind = n.kind(), "unhandled top-level stmt");
                    None
                }
            }
        }
    }

    fn try_lower_assign(&self, n: Node, src: &str) -> Option<Stmt> {
        let op_node = n.child_by_field_name("operator")?;
        let op_text = text(op_node, src)?;
        // Note: tree-sitter-r emits the super-assignment operator as the
        // token `<<-`, NOT `<<`. Matching `<<` (as this code once did)
        // silently fails for every super-assignment and lets it fall
        // through to `lower_binary`, which mis-lowers it.
        if !matches!(op_text.as_str(), "<-" | "<<-" | "=" | "->" | "->>") {
            return None;
        }
        let lhs = n.child_by_field_name("lhs")?;
        let rhs = n.child_by_field_name("rhs")?;
        let (target, value) = if matches!(op_text.as_str(), "->" | "->>") {
            (self.lower_expr(rhs, src)?, self.lower_expr(lhs, src)?)
        } else {
            (self.lower_expr(lhs, src)?, self.lower_expr(rhs, src)?)
        };
        // Super-assignment (`<<-`) must be recorded as such so the checker
        // (and AST consumers) can distinguish it from plain assignment.
        // The statement form `x <<- v` lowers to `Stmt::Assign` carrying
        // the marker on the inner `Expr::BinOp` (mirroring how the rest of
        // the AST represents assignment-as-expression).
        let value = if op_text.as_str() == "<<-" {
            // Re-wrap the RHS so the SuperAssign marker survives in a form
            // downstream code already understands.
            let span = self.span(n, src);
            Expr::BinOp {
                op: BinOpKind::SuperAssign,
                lhs: Box::new(target.clone()),
                rhs: Box::new(value),
                span,
            }
        } else {
            value
        };
        Some(Stmt::Assign {
            target,
            value,
            span: self.span(n, src),
        })
    }

    fn lower_if(&self, n: Node, src: &str) -> Option<Stmt> {
        let cond = self.lower_expr(n.child_by_field_name("condition")?, src)?;
        let consequence = n.child_by_field_name("consequence")?;
        let then = self.lower_block(consequence, src);
        let else_ = n
            .child_by_field_name("alternative")
            .map(|alt| self.lower_block(alt, src));
        Some(Stmt::If {
            cond,
            then,
            else_,
            span: self.span(n, src),
        })
    }

    /// Lower an `if_statement` in expression position (e.g. the RHS of
    /// `x <- if (cond) 1L else 2L`). Unlike `lower_if` which produces a
    /// `Stmt::If`, this produces an `Expr::If` whose branches are
    /// lowered as expressions (the last expression in a braced body
    /// becomes the branch's value).
    ///
    /// Side effects in intermediate statements within a braced branch
    /// (e.g. `if (cond) { y <- 1; y + 1 }`) are NOT walked for
    /// diagnostics in this path. Users who need full diagnostics should
    /// use the statement form. The type inference is correct regardless
    /// (the branch type is the last expression's type).
    fn lower_if_expr(&self, n: Node, src: &str) -> Option<Expr> {
        let cond = self.lower_expr(n.child_by_field_name("condition")?, src)?;
        let consequence = n.child_by_field_name("consequence")?;
        let then = self.lower_expr(consequence, src)?;
        let else_ = n
            .child_by_field_name("alternative")
            .and_then(|alt| self.lower_expr(alt, src))
            .map(Box::new);
        Some(Expr::If {
            cond: Box::new(cond),
            then: Box::new(then),
            else_,
            span: self.span(n, src),
        })
    }

    fn lower_for(&self, n: Node, src: &str) -> Option<Stmt> {
        let name = text(n.child_by_field_name("variable")?, src)?;
        let iter = self.lower_expr(n.child_by_field_name("sequence")?, src)?;
        let body = self.lower_block(n.child_by_field_name("body")?, src);
        Some(Stmt::For {
            name,
            iter,
            body,
            span: self.span(n, src),
        })
    }

    fn lower_while(&self, n: Node, src: &str, repeat_: bool) -> Stmt {
        let body = self.lower_block(n.child_by_field_name("body").unwrap_or(n), src);
        let cond = if repeat_ {
            Expr::Logical(true, self.span(n, src))
        } else {
            self.lower_expr(n.child_by_field_name("condition").unwrap_or(n), src)
                .unwrap_or(Expr::Unknown(self.span(n, src)))
        };
        Stmt::While {
            cond,
            body,
            span: self.span(n, src),
        }
    }

    fn lower_repeat(&self, n: Node, src: &str) -> Stmt {
        let body = self.lower_block(n.child_by_field_name("body").unwrap_or(n), src);
        Stmt::While {
            cond: Expr::Logical(true, self.span(n, src)),
            body,
            span: self.span(n, src),
        }
    }

    /// A braced expression used as a statement. Only the last child's value
    /// is kept as the statement; earlier siblings are dropped. This is a
    /// v1 limitation that only bites when a braced block appears in a
    /// nested statement position (e.g. as a function-body branch). At
    /// *top* level, `RParser::parse` splices all children directly into
    /// the statement list (see that function), so `{ a <- 1; b <- 2 }`
    /// at the top of a file preserves both statements.
    fn lower_braced_as_stmt(&self, n: Node, src: &str) -> Option<Stmt> {
        let mut cur = n.walk();
        let mut last: Option<Stmt> = None;
        for ch in n.named_children(&mut cur) {
            last = self.lower_stmt(ch, src);
        }
        last
    }

    fn lower_block(&self, n: Node, src: &str) -> Vec<Stmt> {
        let mut out = Vec::new();
        if n.kind() == "braced_expression" {
            let mut cur = n.walk();
            for ch in n.named_children(&mut cur) {
                if let Some(s) = self.lower_stmt(ch, src) {
                    out.push(s);
                }
            }
        } else if let Some(s) = self.lower_stmt(n, src) {
            out.push(s);
        }
        out
    }

    fn lower_function_def_as_stmt(&self, n: Node, src: &str) -> Option<Stmt> {
        let params = self.lower_params(n.child_by_field_name("parameters")?, src);
        let body = self.lower_block(n.child_by_field_name("body")?, src);
        Some(Stmt::FunctionDef {
            name: None,
            params,
            body,
            span: self.span(n, src),
        })
    }

    fn lower_params(&self, n: Node, src: &str) -> Vec<Param> {
        let mut out = Vec::new();
        let mut cur = n.walk();
        for ch in n.named_children(&mut cur) {
            if ch.kind() == "parameter" {
                let name = text(ch.child_by_field_name("name").unwrap_or(ch), src)
                    .unwrap_or_else(|| "?".into());
                let default = ch
                    .child_by_field_name("default")
                    .and_then(|n| self.lower_expr(n, src));
                out.push(Param {
                    name,
                    default,
                    span: self.span(ch, src),
                });
            }
        }
        out
    }

    fn lower_expr(&self, n: Node, src: &str) -> Option<Expr> {
        match n.kind() {
            "true" => Some(Expr::Logical(true, self.span(n, src))),
            "false" => Some(Expr::Logical(false, self.span(n, src))),
            "null" => Some(Expr::Null(self.span(n, src))),
            "identifier" => Some(Expr::Ident {
                name: text(n, src)?,
                span: self.span(n, src),
            }),
            "integer" => {
                let raw = text(n, src)?;
                let stripped = raw.trim_end_matches('L').trim_end_matches('l');
                let span = self.span(n, src);
                // Integer literals that don't fit `i64` (e.g. `1e5L`,
                // `0x10L` for non-hex, very large values) must NOT cause
                // the whole statement to vanish. Earlier code returned
                // `None` here, and `?`-propagation in `lower_binary` /
                // `try_lower_assign` dropped the enclosing statement
                // entirely. Fall back to a double, then to `Unknown`, but
                // always produce *some* expression.
                if let Ok(v) = stripped.parse::<i64>() {
                    Some(Expr::Integer(v, span))
                } else if let Ok(d) = stripped.parse::<f64>() {
                    Some(Expr::Double(d, span))
                } else {
                    Some(Expr::Unknown(span))
                }
            }
            "float" | "nan" | "inf" => {
                let raw = text(n, src)?;
                let parsed = match raw.as_str() {
                    "Inf" | "inf" => f64::INFINITY,
                    "-Inf" | "-inf" => f64::NEG_INFINITY,
                    "NaN" | "nan" => f64::NAN,
                    s => s.parse::<f64>().ok()?,
                };
                Some(Expr::Double(parsed, self.span(n, src)))
            }
            "complex" => Some(Expr::Unknown(self.span(n, src))),
            "string" => {
                let raw = text(n, src)?;
                Some(Expr::String(
                    unquote_r_string(raw),
                    self.span(n, src),
                ))
            }
            "na" => {
                let raw = text(n, src)?;
                let t = match raw.as_str() {
                    "NA" => crate::types::RType::scalar(crate::types::Mode::Logical),
                    "NA_integer_" => crate::types::RType::scalar(crate::types::Mode::Integer),
                    "NA_real_" => crate::types::RType::scalar(crate::types::Mode::Double),
                    "NA_complex_" => crate::types::RType::scalar(crate::types::Mode::Complex),
                    "NA_character_" => crate::types::RType::scalar(crate::types::Mode::Character),
                    _ => crate::types::RType::unknown(),
                };
                Some(Expr::Na(t, self.span(n, src)))
            }
            "call" => self.lower_call(n, src),
            "binary_operator" => self.lower_binary(n, src),
            "extract_operator" => self.lower_extract(n, src),
            "namespace_operator" => self.lower_namespace(n, src),
            "unary_operator" => self.lower_unary(n, src),
            "subset" => self.lower_index(n, src, IndexKind::Single),
            "subset2" => self.lower_index(n, src, IndexKind::Double),
            "function_definition" => self.lower_function_literal(n, src),
            "parenthesized_expression" => {
                let mut cur = n.walk();
                if let Some(ch) = n.named_children(&mut cur).next() {
                    return self.lower_expr(ch, src);
                }
                None
            }
            "braced_expression" => {
                // Lower the last expression in the block as the block's value.
                let mut cur = n.walk();
                let mut last: Option<Expr> = None;
                for ch in n.named_children(&mut cur) {
                    last = self.lower_expr(ch, src);
                }
                last
            }
            "if_statement" => self.lower_if_expr(n, src),
            _ => {
                tracing::trace!(kind = n.kind(), "unhandled expr");
                Some(Expr::Unknown(self.span(n, src)))
            }
        }
    }

    fn lower_call(&self, n: Node, src: &str) -> Option<Expr> {
        let func = self.lower_expr(n.child_by_field_name("function")?, src)?;
        let args = self.lower_arguments(n.child_by_field_name("arguments"), src);
        Some(Expr::Call {
            func: Box::new(func),
            args,
            span: self.span(n, src),
        })
    }

    fn lower_index(&self, n: Node, src: &str, kind: IndexKind) -> Option<Expr> {
        // subset/subset2 share the same shape as call: `function` + `arguments`.
        let base = self.lower_expr(n.child_by_field_name("function")?, src)?;
        let args = self.lower_arguments(n.child_by_field_name("arguments"), src);
        Some(Expr::Index {
            base: Box::new(base),
            kind,
            args,
            span: self.span(n, src),
        })
    }

    fn lower_arguments(&self, maybe: Option<Node>, src: &str) -> Vec<Arg> {
        let mut out = Vec::new();
        if let Some(args_node) = maybe {
            let mut cur = args_node.walk();
            for arg in args_node.named_children(&mut cur) {
                if arg.kind() == "argument" {
                    out.push(self.lower_arg(arg, src));
                }
            }
        }
        out
    }

    fn lower_arg(&self, n: Node, src: &str) -> Arg {
        let span = self.span(n, src);
        if let Some(name_node) = n.child_by_field_name("name") {
            if let Some(value_node) = n.child_by_field_name("value") {
                let name = text(name_node, src);
                let value = self
                    .lower_expr(value_node, src)
                    .unwrap_or(Expr::Unknown(span));
                return Arg { name, value, span };
            }
        }
        // Positional: there's still a `value` field in tree-sitter-r.
        if let Some(value_node) = n.child_by_field_name("value") {
            if let Some(v) = self.lower_expr(value_node, src) {
                return Arg {
                    name: None,
                    value: v,
                    span,
                };
            }
        }
        Arg {
            name: None,
            value: Expr::Unknown(span),
            span,
        }
    }

    fn lower_binary(&self, n: Node, src: &str) -> Option<Expr> {
        let op_node = n.child_by_field_name("operator")?;
        let op_text = text(op_node, src)?;
        let span = self.span(n, src);
        let lhs = self.lower_expr(n.child_by_field_name("lhs")?, src)?;
        let rhs = self.lower_expr(n.child_by_field_name("rhs")?, src)?;
        let op = match op_text.as_str() {
            "+" => BinOpKind::Add,
            "-" => BinOpKind::Sub,
            // `**` is R's alternate spelling of `^` (power), not multiply.
            "*" => BinOpKind::Mul,
            "**" => BinOpKind::Pow,
            "/" => BinOpKind::Div,
            "^" => BinOpKind::Pow,
            "%%" => BinOpKind::Mod,
            "%/%" => BinOpKind::IDiv,
            ":" => BinOpKind::Colon,
            "<" => BinOpKind::Lt,
            "<=" => BinOpKind::Le,
            ">" => BinOpKind::Gt,
            ">=" => BinOpKind::Ge,
            "==" => BinOpKind::Eq,
            "!=" => BinOpKind::Ne,
            "&" => BinOpKind::And,
            "&&" => BinOpKind::AndAnd,
            "|" => BinOpKind::Or,
            "||" => BinOpKind::OrOr,
            "%in%" => BinOpKind::In,
            "|>" => BinOpKind::PipeForward,
            "%>%" => BinOpKind::PipeForward,
            "%T>%" => BinOpKind::PipeTee,
            "%<>%" => BinOpKind::PipeAssign,
            // Assignment operators in expression position (e.g. the
            // inner assignment in `a <- b <- 1L`). These return the
            // assigned value in R, so `infer_binop` returns the RHS
            // type for them. `->` and `->>` are right-to-left, so we
            // swap the operands. tree-sitter-r emits the
            // super-assignment token as `<<-` (not `<<`).
            "<-" | "=" => BinOpKind::Assign,
            "<<-" => BinOpKind::SuperAssign,
            "->" | "->>" => {
                // Right-assigned: `a -> b` is `b <- a`. Swap operands.
                return Some(Expr::BinOp {
                    op: BinOpKind::Assign,
                    lhs: Box::new(rhs),
                    rhs: Box::new(lhs),
                    span,
                });
            }
            _ => {
                tracing::trace!(op = op_text.as_str(), "unknown binary op");
                return Some(Expr::Unknown(span));
            }
        };
        Some(Expr::BinOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
            span,
        })
    }

    fn lower_unary(&self, n: Node, src: &str) -> Option<Expr> {
        let op_node = n.child_by_field_name("operator")?;
        let op_text = text(op_node, src)?;
        // unary_operator in this grammar has fields `operator` + `operand`
        // OR `rhs` depending on version; try both.
        let expr_node = n
            .child_by_field_name("operand")
            .or_else(|| n.child_by_field_name("rhs"))?;
        let expr = self.lower_expr(expr_node, src)?;
        let span = self.span(n, src);
        let op = match op_text.as_str() {
            "-" => UnaryOpKind::Neg,
            "+" => return Some(expr),
            "!" => UnaryOpKind::Not,
            _ => return Some(Expr::Unknown(span)),
        };
        Some(Expr::UnaryOp {
            op,
            expr: Box::new(expr),
            span,
        })
    }

    fn lower_extract(&self, n: Node, src: &str) -> Option<Expr> {
        // `x$y` parses as binary_operator with operator `$`. But
        // tree-sitter-r exposes it as `extract_operator`; the fields mirror
        // binary_operator.
        let base = self.lower_expr(n.child_by_field_name("lhs")?, src)?;
        let rhs = n.child_by_field_name("rhs")?;
        let name = text(rhs, src).unwrap_or_default();
        let span = self.span(n, src);
        Some(Expr::Index {
            base: Box::new(base),
            kind: IndexKind::Dollar,
            args: vec![Arg {
                name: Some(name.clone()),
                value: Expr::Ident { name, span },
                span,
            }],
            span,
        })
    }

    /// Lower a namespace-qualified name: `pkg::fn` or `pkg:::fn`.
    ///
    /// We preserve the full qualified name (`pkg::fn`) in the
    /// `Expr::Ident`. The checker then has two ways to handle it:
    ///
    ///   * In call position (`pkg::fn(args)`), `infer_call` strips the
    ///     `pkg::` prefix for typeshed / FnTable lookups, so
    ///     `stats::rnorm(10)` resolves the same way `rnorm(10)` does.
    ///   * In value position (`x <- pkg::fn`), the checker treats any
    ///     `::`-containing name as an opaque cross-package reference
    ///     and does NOT emit RY010, since we don't model other
    ///     packages' export tables.
    ///
    /// Both `::` (exported) and `:::` (internal/unexported) are
    /// preserved as written so the original spelling is recoverable.
    fn lower_namespace(&self, n: Node, src: &str) -> Option<Expr> {
        let span = self.span(n, src);
        let lhs = match n.child_by_field_name("lhs") {
            Some(lhs) => lhs,
            None => {
                tracing::trace!("namespace_operator without lhs");
                return Some(Expr::Unknown(span));
            }
        };
        let pkg = match text(lhs, src) {
            Some(t) => t,
            None => return Some(Expr::Unknown(span)),
        };
        let rhs = match n.child_by_field_name("rhs") {
            Some(rhs) => rhs,
            None => {
                tracing::trace!("namespace_operator without rhs");
                return Some(Expr::Unknown(span));
            }
        };
        let raw = match text(rhs, src) {
            Some(t) => t,
            None => return Some(Expr::Unknown(span)),
        };
        // For identifier RHS the raw text is the bare name. For a string
        // RHS (e.g. `pkg::"my func"`, used for non-syntactic names) we
        // strip the surrounding quotes, mirroring `lower_expr`'s string
        // handling.
        let name = if rhs.kind() == "string" && raw.len() >= 2 {
            raw[1..raw.len() - 1].to_string()
        } else {
            raw
        };
        // Detect the operator (`::` vs `:::`) by scanning the node's
        // anonymous children. We preserve it so the checker can tell
        // exported (`::`) from internal (`:::`) references if needed,
        // and so the original spelling round-trips through the AST.
        let op = namespace_op(n, src).unwrap_or("::");
        let full_name = format!("{}{}{}", pkg, op, name);
        Some(Expr::Ident {
            name: full_name,
            span,
        })
    }

    fn lower_function_literal(&self, n: Node, src: &str) -> Option<Expr> {
        let params = self.lower_params(n.child_by_field_name("parameters")?, src);
        let body = self.lower_block(n.child_by_field_name("body")?, src);
        Some(Expr::Function {
            params,
            body,
            span: self.span(n, src),
        })
    }
}

fn text(n: Node, src: &str) -> Option<String> {
    n.utf8_text(src.as_bytes()).ok().map(String::from)
}

/// Unquote an R string literal, handling escape sequences and raw
/// strings.
///
/// R has four string forms:
///   * plain: `"a\nb"` / `'a\nb'` -- backslash escapes are processed.
///   * raw:   `r"(a\nb)"` / `R"[...]"` / `r"[DELI](...)DELI"` -- the
///     content between the delimiters is taken literally (no escape
///     processing); the `-(...)` / `-[...]` delimiters are stripped.
///
/// Previously this code stripped only the first and last byte, which
/// silently dropped escapes (`"a\nb"` became the 4-char string
/// `a\nb`, not `a`+newline+`b`) and mishandled raw strings. Correct
/// escape processing matters because column-name matching
/// (`df$"my col"`, `list("a b" = 1)`) and `# ry:` directive parsing
/// depend on the literal value.
fn unquote_r_string(raw: &str) -> String {
    // Raw strings: r"(...)" , r"(...){...}", R"(...)", r"[...]", etc.
    // The opening is r/R followed by an optional dash-delimiter and a
    // ( or [. The matching close is ) or ] followed by the same
    // delimiter (reversed) and a quote.
    if let Some(rest) = raw
        .strip_prefix('r')
        .or_else(|| raw.strip_prefix('R'))
    {
        if let Some(unprocessed) = try_unwrap_raw_string(rest) {
            return unprocessed;
        }
        // Not actually a raw string (e.g. an identifier-looking token);
        // fall through to ordinary processing.
    }
    // Ordinary quoted string: process escapes.
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return raw.to_string();
    }
    let inner = &raw[1..raw.len() - 1];
    process_r_escapes(inner)
}

/// Strip the delimiters of a raw string whose body starts after the
/// leading `r"`/`R"` (so `body` begins at the optional `-delim(` or
/// `(`). Returns the literal content if it parses as a raw string,
/// else None.
fn try_unwrap_raw_string(body: &str) -> Option<String> {
    // Opening sequence: optional delimiter chars, then `(` or `[`.
    // R allows `r"(...)"`, `r"-(...)-"`, `r"--(...)--"`, and the `[`
    // bracket form likewise. We capture the delimiter and the bracket.
    let bytes = body.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (open_bracket, close_bracket) = match bytes[0] {
        b'(' => (b'(', b')'),
        b'[' => (b'[', b']'),
        _ => {
            // delimiter form: dashes then bracket
            let mut i = 0;
            while i < bytes.len() && bytes[i] == b'-' {
                i += 1;
            }
            if i == 0 || i >= bytes.len() {
                return None;
            }
            match bytes[i] {
                b'(' => (b'(', b')'),
                b'[' => (b'[', b']'),
                _ => return None,
            }
        }
    };
    // Find the content start (after the opening bracket) and the
    // matching close. For the simple (no-delimiter) form, the close is
    // the last `)bracket"` sequence. We do a conservative search from
    // the end for the closing `"<close>` .
    let close_quote_seq: &[u8] = &[close_bracket, b'"'];
    if body.len() < close_quote_seq.len() {
        return None;
    }
    // The opening bracket is the FIRST bracket char in body.
    let open_idx = body.find(open_bracket as char)?;
    // The closing sequence is the LAST occurrence of `<close>"`.
    let close_idx = body.rfind(std::str::from_utf8(close_quote_seq).ok()?)?;
    if close_idx <= open_idx {
        return None;
    }
    let content_start = open_idx + 1;
    if content_start >= close_idx {
        return Some(String::new());
    }
    Some(body[content_start..close_idx].to_string())
}

/// Process R string escape sequences. Handles the common cases:
/// `\"`, `\\`, `\n`, `\r`, `\t`, `\b`, `\f`, `\v`, `\0`, `\'`, and
/// `\uXXXX` / `\UXXXXXXXX` (4 or 8 hex digits). Unknown escapes are
/// passed through verbatim (R warns but keeps the backslash), matching
/// R's documented behavior.
fn process_r_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b != b'\\' {
            // Copy one UTF-8 char.
            let ch_len = utf8_char_len(b);
            if let Ok(chunk) = std::str::from_utf8(&bytes[i..i + ch_len]) {
                out.push_str(chunk);
            }
            i += ch_len;
            continue;
        }
        // Escape sequence.
        if i + 1 >= bytes.len() {
            out.push('\\');
            break;
        }
        let next = bytes[i + 1];
        let (replaced, consumed) = match next {
            b'n' => (Some('\n'), 2),
            b'r' => (Some('\r'), 2),
            b't' => (Some('\t'), 2),
            b'b' => (Some('\u{0008}'), 2),
            b'f' => (Some('\u{000C}'), 2),
            b'v' => (Some('\u{000B}'), 2),
            b'0' => (Some('\0'), 2),
            b'a' => (Some('\u{0007}'), 2),
            b'"' => (Some('"'), 2),
            b'\'' => (Some('\''), 2),
            b'\\' => (Some('\\'), 2),
            b'\n' => (None, 2), // physical line continuation: drop
            b'u' => {
                // \uXXXX (exactly 4 hex).
                let hex = std::str::from_utf8(&bytes.get(i + 2..i + 6).unwrap_or(&[]))
                    .unwrap_or("");
                if let Ok(n) = u32::from_str_radix(hex, 16) {
                    if let Some(c) = char::from_u32(n) {
                        (Some(c), 6)
                    } else {
                        (None, 6)
                    }
                } else {
                    (None, 2)
                }
            }
            b'U' => {
                // \UXXXXXXXX (exactly 8 hex).
                let hex = std::str::from_utf8(&bytes.get(i + 2..i + 10).unwrap_or(&[]))
                    .unwrap_or("");
                if let Ok(n) = u32::from_str_radix(hex, 16) {
                    if let Some(c) = char::from_u32(n) {
                        (Some(c), 10)
                    } else {
                        (None, 10)
                    }
                } else {
                    (None, 2)
                }
            }
            b'x' => {
                // \xXX (1-2 hex digits).
                let mut j = i + 2;
                let mut hex = String::new();
                while j < bytes.len() && hex.len() < 2 && bytes[j].is_ascii_hexdigit() {
                    hex.push(bytes[j] as char);
                    j += 1;
                }
                if let Ok(n) = u32::from_str_radix(&hex, 16) {
                    (Some(n as u8 as char), 2 + hex.len())
                } else {
                    (None, 2)
                }
            }
            _ => (None, 2), // unknown escape: keep verbatim below
        };
        match replaced {
            Some(c) => out.push(c),
            None => {
                // Unknown escape or line continuation: copy the backslash
                // and the next byte verbatim (R warns but keeps them).
                if next != b'\n' {
                    out.push('\\');
                    out.push(next as char);
                }
            }
        }
        i += consumed;
    }
    out
}

/// Length in bytes of the UTF-8 character starting with the given lead
/// byte. Used to advance one code point at a time without pulling in a
/// unicode crate.
fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b >> 5 == 0b110 {
        2
    } else if b >> 4 == 0b1110 {
        3
    } else if b >> 3 == 0b11110 {
        4
    } else {
        1 // invalid lead byte; advance one to make progress
    }
}

/// Walk the parse tree and collect spans of `ERROR` and `MISSING` nodes.
///
/// tree-sitter produces a recovered tree for malformed input: regions it
/// could not parse become `ERROR` nodes, and tokens it had to insert to
/// repair the tree become `MISSING` nodes. `root.has_error()` is the cheap
/// "is anything broken" check; this function walks the tree when that is
/// true to extract the individual broken regions for per-node diagnostics.
fn collect_parse_errors(root: tree_sitter::Node) -> Vec<Span> {
    if !root.has_error() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // Pre-order DFS over ALL nodes (named and anonymous). `ERROR` and
    // `MISSING` are node kinds tree-sitter emits specially.
    let mut stack: Vec<tree_sitter::Node> = vec![root];
    while let Some(node) = stack.pop() {
        let kind = node.kind();
        if kind == "ERROR" || node.is_missing() {
            let start = node.start_byte();
            let end = node.end_byte().max(start);
            let pos = node.start_position();
            out.push(Span::new(start, end, pos.row, pos.column));
            // Still descend: nested ERROR/MISSING nodes get their own spans
            // so a single broken region reports each missing token once.
        }
        let mut child_cursor = node.walk();
        for child in node.children(&mut child_cursor) {
            stack.push(child);
        }
    }
    out
}

/// Find the namespace operator token (`::` or `:::`) among a
/// `namespace_operator` node's anonymous children. Returns `None` if
/// neither token is present (malformed input).
fn namespace_op(n: Node, src: &str) -> Option<&'static str> {
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        if child.is_named() {
            continue;
        }
        if let Ok(t) = child.utf8_text(src.as_bytes()) {
            if t == ":::" {
                return Some(":::");
            }
            if t == "::" {
                return Some("::");
            }
        }
    }
    None
}

/// Convert a byte column within a single line to a character column.
///
/// Used by diagnostic rendering when a human-visible column is needed. The
/// previous per-node column computation rescanned the entire file from byte
/// 0 for every AST node (O(n^2) total, 47s on 20k lines); this only ever
/// scans the one line the column lives on.
///
/// `line_start` is the byte offset of the start of the line containing the
/// column, and `byte_col` is the byte offset of the target within that line.
#[allow(dead_code)] // no diagnostic renderer consumes char columns yet
pub(crate) fn byte_col_to_char_col(line: &str, byte_col: usize) -> usize {
    let mut col = 0usize;
    for (b, ch) in line.char_indices() {
        if b >= byte_col {
            break;
        }
        let _ = ch;
        col += 1;
    }
    col
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> SourceFile {
        let mut p = RParser::new().expect("parser init");
        p.parse("test.R", src).expect("parse ok")
    }

    #[test]
    fn parses_simple_assignment() {
        let f = parse("x <- 1L\n");
        assert_eq!(f.stmts.len(), 1);
    }

    #[test]
    fn parses_function_def() {
        let f = parse("f <- function(x = 1L) x + 1\n");
        assert!(matches!(f.stmts.first(), Some(Stmt::Assign { .. })));
    }

    #[test]
    fn parses_if() {
        let f = parse("if (x > 0) print(x) else print(-x)\n");
        assert!(matches!(f.stmts.first(), Some(Stmt::If { .. })));
    }

    #[test]
    fn parses_call_with_named_arg() {
        let f = parse("f(x = 1L, y = 2)\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Call { args, .. })) => {
                assert_eq!(args.len(), 2);
                assert_eq!(args[0].name.as_deref(), Some("x"));
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parses_function_with_default_params() {
        let f = parse("f <- function(x = 1L, y = 2, z = \"a\") { x }\n");
        match f.stmts.first() {
            Some(Stmt::Assign {
                value: Expr::Function { params, .. },
                ..
            }) => {
                assert_eq!(params.len(), 3, "params: {:?}", params);
                assert_eq!(params[0].name, "x");
                assert!(
                    matches!(params[0].default, Some(Expr::Integer(1, _))),
                    "x default: {:?}",
                    params[0].default
                );
                assert_eq!(params[1].name, "y");
                assert!(
                    matches!(params[1].default, Some(Expr::Double(2.0, _))),
                    "y default: {:?}",
                    params[1].default
                );
                assert_eq!(params[2].name, "z");
                assert!(
                    matches!(&params[2].default, Some(Expr::String(s, _)) if s == "a"),
                    "z default: {:?}",
                    params[2].default
                );
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parses_string_as_trailing_expression() {
        // Function body whose only statement is a string literal.
        let f = parse("g <- function() { \"hello\" }\n");
        match f.stmts.first() {
            Some(Stmt::Assign {
                value: Expr::Function { body, .. },
                ..
            }) => {
                assert_eq!(body.len(), 1, "body: {:?}", body);
                assert!(
                    matches!(&body[0], Stmt::Expr(Expr::String(s, _)) if s == "hello"),
                    "body[0]: {:?}",
                    body[0]
                );
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parses_base_r_pipe() {
        // Base-R `|>` lowers to PipeForward.
        let f = parse("c(1, 2, 3) |> mean()\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::PipeForward,
                lhs,
                rhs,
                ..
            })) => {
                assert!(
                    matches!(lhs.as_ref(), Expr::Call { .. }),
                    "lhs should be a Call, got {:?}",
                    lhs
                );
                assert!(
                    matches!(rhs.as_ref(), Expr::Call { .. }),
                    "rhs should be a Call, got {:?}",
                    rhs
                );
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parses_magrittr_pipe() {
        // `%>%` lowers to PipeForward (same semantic as `|>` at v1).
        let f = parse("mtcars %>% subset(cyl == 4)\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::PipeForward,
                lhs,
                rhs,
                ..
            })) => {
                assert!(
                    matches!(lhs.as_ref(), Expr::Ident { name, .. } if name == "mtcars"),
                    "lhs should be Ident(\"mtcars\"), got {:?}",
                    lhs
                );
                match rhs.as_ref() {
                    Expr::Call { func, args, .. } => {
                        assert!(
                            matches!(func.as_ref(), Expr::Ident { name, .. } if name == "subset"),
                            "rhs func should be Ident(\"subset\"), got {:?}",
                            func
                        );
                        assert_eq!(args.len(), 1, "rhs args: {:?}", args);
                    }
                    other => panic!("rhs should be a Call, got {:?}", other),
                }
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn parses_magrittr_tee_pipe() {
        // `%T>%` lowers to PipeTee.
        let f = parse("x %T>% print()\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::PipeTee,
                ..
            })) => {}
            other => panic!("expected PipeTee BinOp, got {:?}", other),
        }
    }

    #[test]
    fn parses_magrittr_assign_pipe() {
        // `%<>%` lowers to PipeAssign.
        let f = parse("x %<>% abs()\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::PipeAssign,
                ..
            })) => {}
            other => panic!("expected PipeAssign BinOp, got {:?}", other),
        }
    }

    #[test]
    fn unknown_special_falls_through() {
        // `%like%` is an arbitrary user-defined infix; it lowers to
        // Unknown (we don't model its semantics).
        let f = parse("y %like% \"foo\"\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Unknown(_))) => {}
            other => panic!("expected Unknown for `%like%`, got {:?}", other),
        }
    }

    #[test]
    fn parses_namespace_operator_double_colon() {
        // `stats::rnorm` lowers to an `Expr::Ident` that preserves the
        // full qualified name. The checker strips the prefix for
        // typeshed lookups (so `stats::rnorm(10)` still resolves to
        // `rnorm`) and treats the bare-identifier form as an opaque
        // cross-package reference (no RY010).
        let f = parse("stats::rnorm\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(name, "stats::rnorm", "expected ident name \"stats::rnorm\"");
            }
            other => panic!("expected Ident for `stats::rnorm`, got {:?}", other),
        }
    }

    #[test]
    fn parses_namespace_operator_triple_colon() {
        // `stats:::foobar` (triple colon, internal access) preserves
        // the `:::` operator so the original spelling round-trips.
        let f = parse("stats:::foobar\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(
                    name, "stats:::foobar",
                    "expected ident name \"stats:::foobar\""
                );
            }
            other => panic!("expected Ident for `stats:::foobar`, got {:?}", other),
        }
    }

    #[test]
    fn parses_namespace_operator_call() {
        // `pkg::fn(args)` is parsed as a `call` whose `function` is a
        // `namespace_operator`; the func Ident carries the full
        // `pkg::fn` name and the checker strips the prefix when
        // resolving the call.
        let f = parse("stats::rnorm(10)\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Call { func, args, .. })) => {
                assert!(
                    matches!(func.as_ref(), Expr::Ident { name, .. } if name == "stats::rnorm"),
                    "expected func Ident(\"stats::rnorm\"), got {:?}",
                    func
                );
                assert_eq!(args.len(), 1, "expected 1 arg, got {:?}", args);
            }
            other => panic!("expected Call for `stats::rnorm(10)`, got {:?}", other),
        }
    }

    #[test]
    fn parses_namespace_operator_with_string_rhs() {
        // Non-syntactic names can be reached via `pkg::"my fn"`; we
        // strip the surrounding quotes when lowering but keep the
        // package prefix in the final qualified name.
        let f = parse("base::\"my fn\"\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(name, "base::my fn", "expected ident name \"base::my fn\"");
            }
            other => panic!("expected Ident for `base::\"my fn\"`, got {:?}", other),
        }
    }

    #[test]
    fn parses_negative_integer_literal_as_unary() {
        // `-1L` lowers to a unary minus applied to a positive integer
        // literal. Type-wise this is equivalent to a negative literal,
        // but the structure (UnaryOp(Neg, Integer(1))) must be preserved
        // so the checker can model R's unary-minus semantics.
        let f = parse("-1L\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            })) => {
                assert!(
                    matches!(expr.as_ref(), Expr::Integer(1, _)),
                    "expected Integer(1) operand, got {:?}",
                    expr
                );
            }
            other => panic!("expected UnaryOp(Neg, Integer(1)), got {:?}", other),
        }
    }

    #[test]
    fn parses_negative_double_literal_as_unary() {
        // `-3.5` lowers to UnaryOp(Neg, Double(3.5)). We assert on the
        // structure (not the exact value) since the value isn't the
        // concern -- the negation wrapping is.
        let f = parse("-3.5\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            })) => {
                assert!(
                    matches!(expr.as_ref(), Expr::Double(_, _)),
                    "expected Double operand, got {:?}",
                    expr
                );
            }
            other => panic!("expected UnaryOp(Neg, Double), got {:?}", other),
        }
    }

    #[test]
    fn parses_neg_colon_groups_negated_left() {
        // R's precedence gives unary `-` higher binding than the `:`
        // sequence operator, so `-1:3` is `(-1):3` (= c(-1,0,1,2,3)),
        // NOT `-(1:3)` (= c(-1,-2,-3)). This is a classic R gotcha; the
        // parser must preserve the `(-1):3` grouping.
        let f = parse("-1:3\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::Colon,
                lhs,
                rhs,
                ..
            })) => {
                // LHS must be the negated literal, not a bare literal.
                assert!(
                    matches!(
                        lhs.as_ref(),
                        Expr::UnaryOp {
                            op: UnaryOpKind::Neg,
                            ..
                        }
                    ),
                    "expected lhs UnaryOp(Neg, ..) so `:` sees -1, got {:?}",
                    lhs
                );
                // RHS must be the positive literal `3` (no negation).
                assert!(
                    matches!(rhs.as_ref(), Expr::Double(_, _)),
                    "expected rhs Double(3), got {:?}",
                    rhs
                );
            }
            other => panic!(
                "expected BinOp(Colon, UnaryOp(Neg, ..), ..) for `-1:3`, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn parses_neg_paren_colon_groups_inner_seq() {
        // `-(1:3)` forces the sequence first via parens, so the negation
        // wraps the whole `1:3`. This must differ structurally from
        // `-1:3` (the previous test).
        let f = parse("-(1:3)\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            })) => {
                assert!(
                    matches!(
                        expr.as_ref(),
                        Expr::BinOp {
                            op: BinOpKind::Colon,
                            ..
                        }
                    ),
                    "expected inner BinOp(Colon, ..), got {:?}",
                    expr
                );
            }
            other => panic!("expected UnaryOp(Neg, BinOp(Colon, ..)), got {:?}", other),
        }
    }

    #[test]
    fn parses_neg_times_int_groups_negated_left() {
        // Unary minus binds tighter than `*`, so `-2L * 3L` is
        // `(-2L) * 3L`, not `-(2L * 3L)`. Either way the type is
        // integer, but the grouping must follow R.
        let f = parse("-2L * 3L\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::BinOp {
                op: BinOpKind::Mul,
                lhs,
                ..
            })) => {
                assert!(
                    matches!(
                        lhs.as_ref(),
                        Expr::UnaryOp {
                            op: UnaryOpKind::Neg,
                            ..
                        }
                    ),
                    "expected lhs UnaryOp(Neg, ..), got {:?}",
                    lhs
                );
            }
            other => panic!("expected BinOp(Mul, UnaryOp(Neg, ..), ..), got {:?}", other),
        }
    }

    #[test]
    fn parses_neg_power_binds_looser_than_pow() {
        // In R `^` binds tighter than unary `-`, so `-2^2` is `-(2^2)`
        // (= -4), NOT `(-2)^2` (= 4). The parser must reflect this.
        let f = parse("-2^2\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::UnaryOp {
                op: UnaryOpKind::Neg,
                expr,
                ..
            })) => {
                assert!(
                    matches!(
                        expr.as_ref(),
                        Expr::BinOp {
                            op: BinOpKind::Pow,
                            ..
                        }
                    ),
                    "expected inner BinOp(Pow, ..) so negation wraps it, got {:?}",
                    expr
                );
            }
            other => panic!("expected UnaryOp(Neg, BinOp(Pow, ..)), got {:?}", other),
        }
    }

    #[test]
    fn string_escape_sequences_are_processed() {
        use super::unquote_r_string;
        assert_eq!(unquote_r_string(r#""a\nb""#), "a\nb");
        assert_eq!(unquote_r_string(r#""\t""#), "\t");
        assert_eq!(unquote_r_string(r#""\\""#), "\\");
        assert_eq!(unquote_r_string(r#""\"""#), "\"");
        assert_eq!(unquote_r_string(r#""a\\b""#), "a\\b");
        // Unknown escape: keep verbatim (R warns but retains the backslash).
        assert_eq!(unquote_r_string(r#""\q""#), r#"\q"#);
    }

    #[test]
    fn string_unicode_escapes() {
        use super::unquote_r_string;
        assert_eq!(unquote_r_string(r#""\u00e9""#), "é");
        // Malformed \u (too few hex): keep verbatim, don't panic.
        assert_eq!(unquote_r_string(r#""\uXY""#), r#"\uXY"#);
    }

    #[test]
    fn string_single_quotes_work() {
        use super::unquote_r_string;
        assert_eq!(unquote_r_string("'abc'"), "abc");
        assert_eq!(unquote_r_string(r"'a\nb'"), "a\nb");
    }

    #[test]
    fn raw_strings_skip_escape_processing() {
        use super::unquote_r_string;
        // r"(...)" -- content is literal, no escape processing.
        assert_eq!(unquote_r_string(r#"r"(a\nb)""#), r"a\nb");
        // R"(...)" (capital) likewise.
        assert_eq!(unquote_r_string(r#"R"(x)""#), "x");
        // r"[...]" bracket form.
        assert_eq!(unquote_r_string(r#"r"[literal]""#), "literal");
    }

    #[test]
    fn string_with_embedded_quote_directive_is_not_a_suppression() {
        // A string literal value like "# noqa" must not be confused with
        // a suppression comment when the parser later reasons about it.
        // The string arm produces an Expr::String with the literal value
        // intact (escapes processed).
        let f = parse(r#"x <- "# noqa"
"#);
        match f.stmts.first() {
            Some(Stmt::Assign {
                value: Expr::String(s, _),
                ..
            }) => assert_eq!(s, "# noqa"),
            other => panic!("expected String assign, got {:?}", other),
        }
    }
}
