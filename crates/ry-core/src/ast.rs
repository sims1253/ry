//! Hand-built AST produced by the tree-sitter parser adapter.
//! This is deliberately smaller than R's full grammar: it models only
//! the constructs v1 cares about (assignments, calls, control flow,
//! literals, operators). Unknown forms are kept as `Expr::Unknown`.

use crate::span::Span;
use crate::types::RType;
use std::path::PathBuf;

/// A top-level R source file is a sequence of statements.
#[derive(Debug, Clone, Default)]
pub struct SourceFile {
    pub path: String,
    /// Native on-disk path, if read from disk. The lossy display `path` of
    /// a non-UTF-8 filename can collide with a real U+FFFD filename.
    pub native_path: Option<PathBuf>,
    /// Original UTF-8 text. The checker slices this string by AST spans
    /// rather than scraping diagnostic prose.
    pub source: String,
    pub stmts: Vec<Stmt>,
    /// Parse errors (tree-sitter `ERROR` / `MISSING` nodes) discovered
    /// during parsing. Each entry's span points at the broken region.
    /// The checker surfaces these as `RY000` (syntax-error) diagnostics so
    /// that malformed input no longer checks "clean".
    pub parse_errors: Vec<Span>,
    /// Report-input boundaries produced before R parsing. Their spans use
    /// original document bytes, just like the R AST spans.
    pub input_issues: Vec<InputIssue>,
    /// Maximal byte spans of invalid UTF-8 sequences found while decoding
    /// this file from disk, located in `source` (which for such files is
    /// a Latin-1 transcoding; see `ry_workspace::read_r_source_decoded`). Empty
    /// for valid UTF-8 and for client-supplied editor buffers, which
    /// arrive as Unicode. R's parser rejects such files with "invalid
    /// multibyte character in parser" unless every invalid byte sits
    /// inside a comment or a `%...%` operator token; the checker applies
    /// the same tolerance when surfacing them as `RY000`.
    pub invalid_utf8: Vec<Span>,
    /// Whether the on-disk bytes started with a UTF-8 BOM (`EF BB BF`),
    /// which survives in `source` as a leading U+FEFF character. R's
    /// parser rejects the file with "unexpected input" at 1:1 in every
    /// execution context except `parse(keep.source = TRUE)` (#474), so
    /// the read boundary sets this and the checker flags it as `RY000`.
    /// False for client-supplied editor buffers.
    pub leading_bom: bool,
    /// Byte spans of the `%...%` special-operator tokens in the file,
    /// sorted by start offset. R's lexer (gram.y `SpecialValue`) scans
    /// these tokens as raw bytes with no multibyte validation, so the
    /// checker's invalid-UTF-8 tolerance treats bytes inside them like
    /// comment bytes: `parse()` in R accepts them (#376).
    pub special_operators: Vec<Span>,
    /// Syntax that tree-sitter's more permissive grammar accepts but
    /// base R's parser rejects at parse time. The first (and currently
    /// only) family is the native-pipe (`|>`) right-hand side: R only
    /// rewrites a call whose function is not syntactically special
    /// (`x |> f(y)`, `x |> pkg::f(y)`), or an extraction chain rooted
    /// at the `_` placeholder (R 4.3+: `x |> _$a`, `x |> _[[i]]`).
    /// Forms like `x |> z[.]` or `x |> { ... }` parse here but error in
    /// R. Like [`SourceFile::parse_errors`], the checker surfaces these
    /// as `RY000`; unlike parse errors the region lowers normally, so
    /// the two lists are kept separate and consumers counting broken
    /// regions (facts, heuristics) are unaffected.
    pub syntax_violations: Vec<SyntaxViolation>,
    /// All `comment` nodes collected during parsing, in source order.
    /// Each entry is the comment's body (the text AFTER the leading
    /// `#`, untrimmed) and its line number (0-indexed). The checker
    /// uses these for lexical suppression parsing, so a `#` that
    /// appears INSIDE a string literal is NOT mistaken for a comment.
    pub comments: Vec<Comment>,
    /// Exact CST function ranges and any braced body ranges. Annotation
    /// readers first establish the innermost lexical function, including
    /// unbraced functions, before deciding whether a comment is in its body.
    pub function_bodies: Vec<FunctionBody>,
}

impl SourceFile {
    /// The native path, which differs from the display path for non-UTF-8
    /// filenames; falls back to the display path for unsaved sources.
    pub fn native_or_display_path(&self) -> &std::path::Path {
        self.native_path
            .as_deref()
            .unwrap_or(std::path::Path::new(&self.path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputIssue {
    pub span: Span,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FunctionBody {
    pub function: Span,
    pub body: Option<Span>,
}

/// A source comment, collected lexically by the parser. `body` excludes
/// the leading `#`.
#[derive(Debug, Clone)]
pub struct Comment {
    /// 0-indexed line of the comment's first character.
    pub line: usize,
    /// 0-indexed byte column of the comment's first character (`#`).
    /// A column of 0 means the comment starts the line (standalone);
    /// a column > 0 means code precedes it on the line (trailing). The
    /// suppression parser uses this to distinguish a standalone
    /// `# ry: ignore` (applies to the NEXT line) from a trailing
    /// `x <- bad  # ry: ignore` (applies to THIS line).
    pub col: usize,
    /// Comment text after the leading `#` (not trimmed; the suppression
    /// parser trims as needed).
    pub body: String,
}

/// A construct that tree-sitter parses but base R's parser rejects at
/// parse time (see [`SourceFile::syntax_violations`]). `message` is the
/// full diagnostic text; it mirrors base R's own parse errors so R users
/// recognize the wording.
#[derive(Debug, Clone)]
pub struct SyntaxViolation {
    pub span: Span,
    pub message: String,
}

#[derive(Debug)]
pub enum Stmt {
    /// `target <- value`
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
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
        /// Span of just the loop-variable identifier (`nm`), as opposed
        /// to `span`, the whole statement. Consumers that highlight or
        /// rename the binding (LSP navigation) use this so they do not
        /// accidentally cover `iter` / `body`.
        name_span: Span,
        iter: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `while (cond) body` / `repeat body`
    While {
        cond: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `function(params) body`
    FunctionDef {
        params: Vec<Param>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `return(value)` / `invisible(value)`
    Return { value: Option<Expr>, span: Span },
}

// Hand-written so an `else if` chain clones on a growable stack.
impl Clone for Stmt {
    fn clone(&self) -> Self {
        match self {
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: target.clone(),
                value: value.clone(),
                span: *span,
            },
            Stmt::Expr(expression) => Stmt::Expr(expression.clone()),
            Stmt::If {
                cond,
                then,
                else_,
                span,
            } => crate::stack::grow(|| Stmt::If {
                cond: cond.clone(),
                then: then.clone(),
                else_: else_.clone(),
                span: *span,
            }),
            Stmt::For {
                name,
                name_span,
                iter,
                body,
                span,
            } => Stmt::For {
                name: name.clone(),
                name_span: *name_span,
                iter: iter.clone(),
                body: body.clone(),
                span: *span,
            },
            Stmt::While { cond, body, span } => Stmt::While {
                cond: cond.clone(),
                body: body.clone(),
                span: *span,
            },
            Stmt::FunctionDef { params, body, span } => Stmt::FunctionDef {
                params: params.clone(),
                body: body.clone(),
                span: *span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.clone(),
                span: *span,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug)]
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
    Call {
        func: Box<Expr>,
        args: Vec<Arg>,
        span: Span,
    },
    /// Identifier reference.
    Ident { name: String, span: Span },
    /// Binary operator: `a + b`, `a %>% b`, etc.
    BinOp {
        op: BinOpKind,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// Unary op: `-x`, `!x`
    UnaryOp {
        op: UnaryOpKind,
        expr: Box<Expr>,
        span: Span,
    },
    /// Subset: `x[i]`, `x[[i]]`, `x$i`, `x[i, j]`
    Index {
        base: Box<Expr>,
        kind: IndexKind,
        args: Vec<Arg>,
        span: Span,
    },
    /// Function literal (anonymous), used as a value.
    Function {
        params: Vec<Param>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// Braced expression used as a value, e.g. `test_that("x", { ... })`
    /// or `x <- { a <- 1; a + 1 }`. Statements execute sequentially in
    /// the current scope and the block's value is the final statement.
    Block { body: Vec<Stmt>, span: Span },
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
    /// An omitted actual argument, such as `f(, x)` or `f(x = )`.
    /// Its position and optional name remain in the enclosing `Arg`.
    Missing(Span),
    /// Anything we don't model yet.
    Unknown(Span),
}

// Hand-written so a long binary-operator chain clones on a growable
// stack: `1 + 1 + ...` and `x |> f() |> g()` are as deep as they are
// long, and a pipe injects a copy of that chain as its first argument
// (#842).
impl Clone for Expr {
    fn clone(&self) -> Self {
        crate::stack::grow(|| self.clone_inner())
    }
}

/// Left spines are unbounded by the syntax nesting limit (see
/// `lower_binary`), while Rust's drop glue recurses once per `Box<Expr>`.
/// Take a deep spine apart iteratively instead, so a long-but-admissible
/// operator chain cannot overflow a worker stack on teardown (#842).
impl Drop for Expr {
    fn drop(&mut self) {
        if !has_long_left_spine(self) {
            return;
        }
        let mut pending = Vec::new();
        take_child_expressions(self, &mut pending);
        while let Some(mut expression) = pending.pop() {
            take_child_expressions(&mut expression, &mut pending);
            // `expression` drops here with no expression children left.
        }
    }
}

/// Whether `e` is a binary chain of at least [`DEEP_SPINE_PROBE`] terms.
/// The probe is bounded so ordinary nodes pay only a few comparisons.
fn has_long_left_spine(e: &Expr) -> bool {
    const DEEP_SPINE_PROBE: usize = 64;
    let mut current = e;
    for _ in 0..DEEP_SPINE_PROBE {
        match current {
            Expr::BinOp { lhs, rhs, .. } => {
                // Rightward assignment (`a -> b`) lowers with swapped
                // operands: the target sits on the left, so continue down
                // the left only for a real `BinOp` and otherwise take the
                // right-hand continuation. A call target on the left is
                // not the chain.
                if matches!(**lhs, Expr::BinOp { .. }) {
                    current = lhs;
                } else if is_chain_node(rhs) {
                    current = rhs;
                } else {
                    return false;
                }
            }
            // A user-defined infix operator (`a %foo% b`) lowers to a call
            // and nests its chain in the first argument; an ordinary call
            // is syntax-bounded.
            Expr::Call { func, args, .. } if matches!(**func, Expr::Ident { ref name, .. } if name.starts_with('%')) => {
                match args.first() {
                    Some(argument) => current = &argument.value,
                    None => return false,
                }
            }
            _ => return false,
        }
    }
    true
}

/// Whether `e` can continue a chain (a binary node or a lowered user-infix
/// call).
fn is_chain_node(e: &Expr) -> bool {
    match e {
        Expr::BinOp { .. } => true,
        Expr::Call { func, .. } => {
            matches!(**func, Expr::Ident { ref name, .. } if name.starts_with('%'))
        }
        _ => false,
    }
}

/// Move every immediate expression child of `e` into `out`, leaving a
/// cheap placeholder behind so `e`'s own drop stays shallow.
fn take_child_expressions(e: &mut Expr, out: &mut Vec<Expr>) {
    fn take(slot: &mut Expr, out: &mut Vec<Expr>) {
        out.push(std::mem::replace(
            slot,
            Expr::Unknown(crate::Span::new(0, 0, 0, 0)),
        ));
    }
    match e {
        Expr::Call { func, args, .. } => {
            take(func, out);
            for argument in args {
                take(&mut argument.value, out);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            take(lhs, out);
            take(rhs, out);
        }
        Expr::UnaryOp { expr, .. } => take(expr, out),
        Expr::Index { base, args, .. } => {
            take(base, out);
            for argument in args {
                take(&mut argument.value, out);
            }
        }
        Expr::Function { params, .. } => {
            for param in params {
                if let Some(default) = &mut param.default {
                    take(default, out);
                }
            }
        }
        Expr::If {
            cond, then, else_, ..
        } => {
            take(cond, out);
            take(then, out);
            if let Some(alternative) = else_ {
                take(alternative, out);
            }
        }
        // A block body holds statements whose own drops flatten their
        // chains; statement nesting stays under the syntax depth limit.
        Expr::Block { .. } => {}
        // No expression children.
        Expr::Logical(..)
        | Expr::Integer(..)
        | Expr::Double(..)
        | Expr::String(..)
        | Expr::Null(..)
        | Expr::Na(..)
        | Expr::Ident { .. }
        | Expr::Missing(..)
        | Expr::Unknown(..) => {}
    }
}

impl Expr {
    fn clone_inner(&self) -> Expr {
        match self {
            Expr::Logical(value, span) => Expr::Logical(*value, *span),
            Expr::Integer(value, span) => Expr::Integer(*value, *span),
            Expr::Double(value, span) => Expr::Double(*value, *span),
            Expr::String(value, span) => Expr::String(value.clone(), *span),
            Expr::Null(span) => Expr::Null(*span),
            Expr::Na(value, span) => Expr::Na(value.clone(), *span),
            Expr::Call { func, args, span } => Expr::Call {
                func: func.clone(),
                args: args.clone(),
                span: *span,
            },
            Expr::Ident { name, span } => Expr::Ident {
                name: name.clone(),
                span: *span,
            },
            Expr::BinOp { op, lhs, rhs, span } => Expr::BinOp {
                op: *op,
                lhs: lhs.clone(),
                rhs: rhs.clone(),
                span: *span,
            },
            Expr::UnaryOp { op, expr, span } => Expr::UnaryOp {
                op: *op,
                expr: expr.clone(),
                span: *span,
            },
            Expr::Index {
                base,
                kind,
                args,
                span,
            } => Expr::Index {
                base: base.clone(),
                kind: *kind,
                args: args.clone(),
                span: *span,
            },
            Expr::Function { params, body, span } => Expr::Function {
                params: params.clone(),
                body: body.clone(),
                span: *span,
            },
            Expr::Block { body, span } => Expr::Block {
                body: body.clone(),
                span: *span,
            },
            Expr::If {
                cond,
                then,
                else_,
                span,
            } => Expr::If {
                cond: cond.clone(),
                then: then.clone(),
                else_: else_.clone(),
                span: *span,
            },
            Expr::Missing(span) => Expr::Missing(*span),
            Expr::Unknown(span) => Expr::Unknown(*span),
        }
    }
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
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Mod,
    IDiv,
    /// `:` sequence operator (`from:to`)
    Colon,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    AndAnd,
    Or,
    OrOr,
    In,
    Assign,
    SuperAssign,
    /// `%>%` (magrittr). `lhs %>% rhs` calls `rhs` with `lhs` prepended
    /// to its positional arguments, or substituted into the `.`
    /// placeholder. Unlike [`BinOpKind::PipeNative`]'s single `_`
    /// insertion point, `.` may appear any number of times anywhere in
    /// the RHS (`x %>% paste(., ., sep = "-")`, `x %>% sum(rev(.))`);
    /// only a top-level `.` argument suppresses the prepended LHS.
    PipeForward,
    /// `|>` (base R 4.1+). Inserts the LHS as the first argument of the
    /// RHS call. R 4.2+ additionally allows `_` once, and only as a named
    /// argument (`x |> f(y = _)`) — never positionally or nested inside an
    /// inner call — and R 4.3+ allows it at the root of an extraction
    /// chain (`x |> _$col`). Its placeholder is `_`, never magrittr's `.`.
    PipeNative,
    /// `%T>%` (magrittr tee pipe). Returns the LHS, ignoring RHS.
    PipeTee,
    /// `%<>%` (magrittr assignment pipe). `x %<>% f()` is `x <- x %>% f()`.
    /// Modeled as PipeForward for the result type; the assignment
    /// side-effect is the caller's responsibility (see checker comment).
    PipeAssign,
}

impl BinOpKind {
    /// Whether `self` is one of R's seven arithmetic operators (the Arith
    /// members without the group dispatch machinery): `+ - * / ^ %% %/%`.
    /// Comparison, logic, and sequence operators are excluded.
    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Pow | Self::Mod | Self::IDiv
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOpKind {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexKind {
    /// `x[i]`
    Single,
    /// `x[[i]]`
    Double,
    /// `x$i`
    Dollar,
    /// `x@i`
    Slot,
}
