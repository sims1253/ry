//! Parser correctness regression tests.
//!
//! Each test pins a past parser bug so it cannot return.

use ry_core::RParser;
use ry_core::ast::{BinOpKind, Expr, Stmt};

fn parse(src: &str) -> ry_core::ast::SourceFile {
    let mut p = RParser::new().expect("parser init");
    p.parse("parser_correctness.R", src).expect("parse")
}

/// Regression: `<<-` was once unrecognized (the lowering matched the
/// string `"<<"`, but tree-sitter-r emits `<<-`). A super-assignment must lower to
/// `Stmt::Assign` (or otherwise be recognized as a super-assignment), not be
/// dropped or mis-lowered.
#[test]
fn super_assignment_is_recognized() {
    let file = parse("x <<- 1\n");
    let kinds: Vec<&Stmt> = file.stmts.iter().collect();
    assert!(
        kinds.iter().any(|s| matches!(s, Stmt::Assign { .. })),
        "x <<- 1 must lower to a Stmt::Assign (super-assignment); got {:?}",
        file.stmts
    );
    // And specifically: the assignment must be a *super*-assignment, not a
    // plain one. The current bug lowers `<<` to `BinOpKind::Assign`.
    let is_super = file.stmts.iter().any(|s| match s {
        Stmt::Assign { value, .. } => matches!(
            value,
            Expr::BinOp {
                op: BinOpKind::SuperAssign,
                ..
            }
        ),
        _ => false,
    });
    assert!(
        is_super,
        "x <<- 1 must be recognized as SuperAssign; got {:?}",
        file.stmts
    );
}

#[test]
fn statement_level_walrus_bind_is_recognized() {
    let file = parse("Person := new_class()\nPerson\n");
    assert!(
        matches!(
            file.stmts.first(),
            Some(Stmt::Assign {
                target: Expr::Ident { name, .. },
                ..
            }) if name == "Person"
        ),
        "bare statement-level := must introduce its identifier: {:?}",
        file.stmts
    );
}

#[test]
fn nested_walrus_expression_is_not_plain_assignment() {
    let file = parse("mutate(df, !!name := value)\n");
    assert!(
        file.stmts
            .iter()
            .all(|statement| !matches!(statement, Stmt::Assign { .. })),
        "tidy-eval := inside a call must not become a top-level assignment: {:?}",
        file.stmts
    );
}

/// Regression: `**` was once mapped to `Mul`. In R
/// `**` is `^` (power), so it must lower to `BinOpKind::Pow`.
#[test]
fn star_star_is_pow() {
    let file = parse("2 ** 3\n");
    let pow = file.stmts.iter().any(|s| match s {
        Stmt::Expr(Expr::BinOp { op, .. }) => *op == BinOpKind::Pow,
        _ => false,
    });
    assert!(pow, "2 ** 3 must lower to Pow; got {:?}", file.stmts);
}

/// Regression: integer literals that fail `i64` parse (`1e5L`,
/// `0x10L`) return `None`, and `?`-propagation in `lower_binary` /
/// `try_lower_assign` silently deletes the whole enclosing statement. The
/// statement must NOT vanish: `n <- 1e5L` and `m <- n + 1` must both survive.
#[test]
fn failed_integer_literal_does_not_drop_statement() {
    let file = parse("n <- 1e5L\nm <- n + 1\n");
    assert_eq!(
        file.stmts.len(),
        2,
        "both statements must be preserved; got {:?}",
        file.stmts
    );
}

/// Regression: `lower_braced_as_stmt` keeps
/// only the last statement of a top-level `{ ... }` block. All statements
/// must be preserved.
#[test]
fn top_level_braced_block_preserves_all_statements() {
    let file = parse("{ a <- 1\nb <- 2\n}\n");
    // Either two separate top-level statements, or a single block carrying
    // both. Today only the last survives; this asserts both are kept.
    let count = file
        .stmts
        .iter()
        .map(|s| match s {
            Stmt::Assign { .. } => 1,
            _ => 0,
        })
        .sum::<usize>();
    assert_eq!(
        count, 2,
        "top-level {{ a <- 1; b <- 2 }} must preserve both assignments; got {:?}",
        file.stmts
    );
}

#[test]
fn user_infix_preserves_operator_and_operands() {
    let file = parse("left %custom% right\n");
    assert!(
        matches!(
            file.stmts.first(),
            Some(Stmt::Expr(Expr::Call { func, args, .. }))
                if matches!(func.as_ref(), Expr::Ident { name, .. } if name == "%custom%")
                    && matches!(&args[0].value, Expr::Ident { name, .. } if name == "left")
                    && matches!(&args[1].value, Expr::Ident { name, .. } if name == "right")
        ),
        "user infix operands must survive lowering: {:?}",
        file.stmts
    );
}

/// Regression for the UTF-8 boundary panic in `lower_namespace`.
///
/// When the RHS of a `::`/`:::` node is a string token whose last byte
/// falls inside a multi-byte character (e.g. an unterminated string with
/// a multibyte char), the slice `raw[1..raw.len() - 1]` panics because
/// `raw.len() - 1` is not a char boundary.  The companion
/// `unquote_r_string` already walks back to the nearest char boundary
/// for the identical class of input; this test verifies `lower_namespace`
/// does the same.
#[test]
fn namespace_string_rhs_multibyte_no_panic() {
    let mut p = ry_core::RParser::new().expect("parser init");
    // Input: `a::"\nÿ` — a namespace operator whose RHS is a string
    // containing a backslash-n escape followed by the two-byte UTF-8
    // character ÿ (U+00FF), with no closing quote.  Tree-sitter
    // produces a `string` node whose raw text is `"\nÿ` (5 bytes);
    // `raw.len() - 1` = 4 lands inside ÿ (bytes 3–4), causing a panic.
    let src = "a::\"\\nÿ";
    let file = p
        .parse("utf8_boundary.R", src)
        .expect("parse must not panic");
    // The parser must return a result, not panic.  The exact AST for
    // malformed input may vary; we only assert no panic here.
    assert!(
        !file.stmts.is_empty(),
        "parser must produce at least one statement"
    );
}

/// Well-formed multibyte namespace strings produce the correct name.
#[test]
fn namespace_string_rhs_multibyte_well_formed() {
    let file = parse("pkg::\"ÿ\"\n");
    assert!(
        file.stmts
            .iter()
            .any(|s| matches!(s, Stmt::Expr(Expr::Ident { name, .. }) if name == "pkg::ÿ")),
        "pkg::\"ÿ\" must produce Ident {{ name: \"pkg::ÿ\" }}; got {:?}",
        file.stmts
    );
}

/// Minimized fuzz crash input — three-byte UTF-8 character in
/// an unterminated namespace string RHS.
#[test]
fn namespace_string_rhs_three_byte_unterminated_no_panic() {
    let mut p = ry_core::RParser::new().expect("parser init");
    // `\n` (backslash-n) followed by 中 (U+4E2D, three bytes 0xE4 0xB8 0xAD).
    let src = "a::\"\\n中";
    let file = p
        .parse("utf8_boundary.R", src)
        .expect("parse must not panic");
    assert!(
        !file.stmts.is_empty(),
        "parser must produce at least one statement"
    );
}

/// Four-byte UTF-8 character (emoji) in an unterminated
/// namespace string RHS must not panic either.
#[test]
fn namespace_string_rhs_four_byte_unterminated_no_panic() {
    let mut p = ry_core::RParser::new().expect("parser init");
    // `\n` followed by 😀 (U+1F600, four bytes 0xF0 0x9F 0x98 0x80).
    let src = "a::\"\\n😀";
    let file = p
        .parse("utf8_boundary.R", src)
        .expect("parse must not panic");
    assert!(
        !file.stmts.is_empty(),
        "parser must produce at least one statement"
    );
}
