//! Parser correctness regression tests (PLAN.md Phase 0.3).
//!
//! These pin four known parser bugs. They are EXPECTED TO FAIL against the
//! current code; they turn green as each bug is fixed in Phase 1.4 / Phase 4.
//! Each test names the PLAN.md finding it tracks.

use ry_core::ast::{BinOpKind, Expr, Stmt};
use ry_core::RParser;

fn parse(src: &str) -> ry_core::ast::SourceFile {
    let mut p = RParser::new().expect("parser init");
    p.parse("parser_correctness.R", src).expect("parse")
}

/// PLAN finding 6, bullet 1: `<<-` is unrecognized. `try_lower_assign`
/// (`parser.rs:108`) and `lower_binary` (`parser.rs:439`) match the string
/// `"<<"`, but tree-sitter-r emits `<<-`. A super-assignment must lower to
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

/// PLAN finding 6, bullet 2: `**` is mapped to `Mul` (`parser.rs:413`). In R
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

/// PLAN finding 6, bullet 3: integer literals that fail `i64` parse (`1e5L`,
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

/// PLAN finding 6, bullet 4: `lower_braced_as_stmt` (`parser.rs:207`) keeps
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
