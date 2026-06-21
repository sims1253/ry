//! Hand-built AST produced by the tree-sitter parser adapter.
//! This is deliberately smaller than R's full grammar: it models only
//! the constructs v1 cares about (assignments, calls, control flow,
//! literals, operators). Unknown forms are kept as `Expr::Unknown`.

use crate::span::Span;
use crate::types::RType;

/// A top-level R source file is a sequence of statements.
#[derive(Debug, Clone, Default)]
pub struct SourceFile {
    pub path: String,
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    /// `target <- value`
    Assign { target: Expr, value: Expr, span: Span },
    /// Bare expression as a statement.
    Expr(Expr),
    /// `if (cond) then [else else_]`
    If {
        cond: Expr,
        then: Vec<Stmt>,
        else_: Option<Vec<Stmt>>,
        span: Span,
    },
    /// `for (nm in iter) body`
    For {
        name: String,
        iter: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `while (cond) body` / `repeat body`
    While { cond: Expr, body: Vec<Stmt>, span: Span },
    /// `function(params) body`
    FunctionDef {
        name: Option<String>,
        params: Vec<Param>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `return(value)` / `invisible(value)`
    Return { value: Option<Expr>, span: Span },
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Expr {
    /// `TRUE`/`FALSE`
    Logical(bool, Span),
    /// `1L`, `1L:10L`
    Integer(i64, Span),
    /// `1.5`, `1e10`
    Double(f64, Span),
    /// `"foo"`
    String(String, Span),
    /// `NULL`
    Null(Span),
    /// `NA`, `NA_real_`, `NA_integer_`, `NA_character_`
    Na(RType, Span),
    /// `c(...)`
    Call { func: Box<Expr>, args: Vec<Arg>, span: Span },
    /// Identifier reference.
    Ident { name: String, span: Span },
    /// Binary operator: `a + b`, `a %>% b`, etc.
    BinOp { op: BinOpKind, lhs: Box<Expr>, rhs: Box<Expr>, span: Span },
    /// Unary op: `-x`, `!x`
    UnaryOp { op: UnaryOpKind, expr: Box<Expr>, span: Span },
    /// Subset: `x[i]`, `x[[i]]`, `x$i`, `x[i, j]`
    Index { base: Box<Expr>, kind: IndexKind, args: Vec<Arg>, span: Span },
    /// Function literal (anonymous), used as a value.
    Function { params: Vec<Param>, body: Vec<Stmt>, span: Span },
    /// Conditional expression: `if (cond) expr1 else expr2`. Used in
    /// expression position (e.g. `x <- if (cond) 1L else 2L`). The
    /// result type is the join of the two branches. `else_` is `None`
    /// when the `else` clause is absent (R returns NULL in that case).
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        else_: Option<Box<Expr>>,
        span: Span,
    },
    /// Anything we don't model yet.
    Unknown(Span),
}

#[derive(Debug, Clone)]
pub struct Arg {
    /// `name = value` if named, otherwise positional.
    pub name: Option<String>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOpKind {
    Add, Sub, Mul, Div, Pow, Mod, IDiv,
    /// `:` sequence operator (`from:to`)
    Colon,
    Lt, Le, Gt, Ge, Eq, Ne,
    And, AndAnd, Or, OrOr,
    NotIn, In,
    Assign, SuperAssign,
    /// `|>` (base R 4.1+) and `%>%` (magrittr). Both desugar the same
    /// way at v1: `lhs |> rhs` calls `rhs` with `lhs` prepended to its
    /// positional arguments (or substituted into the placeholder).
    PipeForward,
    /// `%T>%` (magrittr tee pipe). Returns the LHS, ignoring RHS.
    PipeTee,
    /// `%<>%` (magrittr assignment pipe). `x %<>% f()` is `x <- x %>% f()`.
    /// Modeled as PipeForward for the result type; the assignment
    /// side-effect is the caller's responsibility (see checker comment).
    PipeAssign,
    /// `%>_%` / placeholder-free magrittr binding (kept for symmetry).
    PipeBind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOpKind {
    Neg, Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    /// `x[i]`
    Single,
    /// `x[[i]]`
    Double,
    /// `x$i`
    Dollar,
}
