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
    TreeSitter { line: usize, col: usize, message: String },
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
        let tree = self.parser.parse(src, None).ok_or_else(|| {
            ParseError::TreeSitter {
                line: 0,
                col: 0,
                message: "parser returned no tree".into(),
            }
        })?;
        let root = tree.root_node();
        let mut stmts = Vec::new();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            if let Some(stmt) = self.lower_stmt(child, src) {
                stmts.push(stmt);
            }
        }
        Ok(SourceFile {
            path: path.to_string(),
            stmts,
        })
    }

    fn span(&self, n: Node, src: &str) -> Span {
        let start = n.start_byte();
        let end = n.end_byte();
        let line = n.start_position().row;
        let col = char_col(src, start);
        Span::new(start, end, line, col)
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
        if !matches!(op_text.as_str(), "<-" | "<<" | "=" | "->" | "->>") {
            return None;
        }
        let lhs = n.child_by_field_name("lhs")?;
        let rhs = n.child_by_field_name("rhs")?;
        let (target, value) = if matches!(op_text.as_str(), "->" | "->>") {
            (self.lower_expr(rhs, src)?, self.lower_expr(lhs, src)?)
        } else {
            (self.lower_expr(lhs, src)?, self.lower_expr(rhs, src)?)
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
        let else_ = n.child_by_field_name("alternative").map(|alt| self.lower_block(alt, src));
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

    /// A braced expression used as a top-level statement. We splice the
    /// *last* child as the statement, losing earlier siblings. This is a
    /// known v1 limitation; at top level braces are rare and a proper
    /// Block variant is a future task.
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
                stripped.parse::<i64>().ok().map(|v| Expr::Integer(v, self.span(n, src)))
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
                let unquoted = &raw[1..raw.len().saturating_sub(1)];
                Some(Expr::String(unquoted.to_string(), self.span(n, src)))
            }
            "na" => {
                let raw = text(n, src)?;
                let t = match raw.as_str() {
                    "NA" => crate::types::RType::scalar(crate::types::Mode::Logical, true),
                    "NA_integer_" => crate::types::RType::scalar(crate::types::Mode::Integer, true),
                    "NA_real_" => crate::types::RType::scalar(crate::types::Mode::Double, true),
                    "NA_complex_" => {
                        crate::types::RType::scalar(crate::types::Mode::Complex, true)
                    }
                    "NA_character_" => {
                        crate::types::RType::scalar(crate::types::Mode::Character, true)
                    }
                    _ => crate::types::RType::UNKNOWN,
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
                return Arg { name: None, value: v, span };
            }
        }
        Arg { name: None, value: Expr::Unknown(span), span }
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
            "*" | "**" => BinOpKind::Mul,
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
            // swap the operands.
            "<-" | "=" | "<<" => BinOpKind::Assign,
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
    /// We drop the package prefix and lower to a plain `Expr::Ident`
    /// named after the RHS, so the typeshed can resolve it directly.
    /// This matches how `pkg::fn()` is typically used in modern R:
    /// `stats::lm(x ~ y)` resolves the same way `lm(x ~ y)` does for
    /// type-checking purposes (the namespace just selects the binding).
    /// Both `::` (exported) and `:::` (internal/unexported) are handled
    /// identically, since v1 doesn't track export visibility.
    fn lower_namespace(&self, n: Node, src: &str) -> Option<Expr> {
        let span = self.span(n, src);
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
        Some(Expr::Ident { name, span })
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

fn char_col(src: &str, byte_offset: usize) -> usize {
    let mut col = 0;
    for (b, ch) in src.char_indices() {
        if b >= byte_offset {
            break;
        }
        if ch == '\n' {
            col = 0;
        } else {
            col += 1;
        }
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
        // `stats::rnorm` lowers to a bare `Expr::Ident { name: "rnorm" }`,
        // dropping the package prefix.
        let f = parse("stats::rnorm\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(name, "rnorm", "expected ident name \"rnorm\"");
            }
            other => panic!("expected Ident for `stats::rnorm`, got {:?}", other),
        }
    }

    #[test]
    fn parses_namespace_operator_triple_colon() {
        // `stats:::foobar` (triple colon, internal access) lowers the
        // same way as `::` for type-checking purposes.
        let f = parse("stats:::foobar\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(name, "foobar", "expected ident name \"foobar\"");
            }
            other => panic!(
                "expected Ident for `stats:::foobar`, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn parses_namespace_operator_call() {
        // `pkg::fn(args)` is parsed as a `call` whose `function` is a
        // `namespace_operator`; we resolve it to `fn(args)` for typing.
        let f = parse("stats::rnorm(10)\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Call { func, args, .. })) => {
                assert!(
                    matches!(func.as_ref(), Expr::Ident { name, .. } if name == "rnorm"),
                    "expected func Ident(\"rnorm\"), got {:?}",
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
        // strip the surrounding quotes when lowering.
        let f = parse("base::\"my fn\"\n");
        match f.stmts.first() {
            Some(Stmt::Expr(Expr::Ident { name, .. })) => {
                assert_eq!(name, "my fn", "expected ident name \"my fn\"");
            }
            other => panic!(
                "expected Ident for `base::\"my fn\"`, got {:?}",
                other
            ),
        }
    }
}
