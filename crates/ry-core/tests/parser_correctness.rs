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

/// Unsupported numeric spellings must not drop their enclosing statement.
#[test]
fn failed_integer_literal_does_not_drop_statement() {
    let file = parse("n <- 0x1p2L\nm <- n + 1\n");
    assert_eq!(
        file.stmts.len(),
        2,
        "both statements must be preserved; got {:?}",
        file.stmts
    );
    assert!(matches!(
        &file.stmts[0],
        Stmt::Assign {
            value: Expr::Unknown(_),
            ..
        }
    ));
}

#[test]
fn integer_suffix_uses_r_storage_range_and_value() {
    for (source, expected) in [
        ("1e5L", 100000),
        ("0x10L", 16),
        ("2147483647L", 2147483647),
        ("1.0L", 1),
    ] {
        let file = parse(source);
        assert!(
            matches!(&file.stmts[..], [Stmt::Expr(Expr::Integer(value, _))] if *value == expected),
            "{source}: {:?}",
            file.stmts
        );
    }
    for (source, expected) in [
        ("2147483648L", 2147483648.0),
        ("9007199254740993L", 9007199254740992.0),
        ("1.5L", 1.5),
        ("0x80000000L", 2147483648.0),
        ("1e100L", 1e100),
    ] {
        let file = parse(source);
        assert!(
            matches!(&file.stmts[..], [Stmt::Expr(Expr::Double(value, _))] if *value == expected),
            "{source}: {:?}",
            file.stmts
        );
    }
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

#[test]
fn omitted_actuals_preserve_every_position_and_name() {
    for (source, expected) in [
        ("f()", vec![]),
        ("f(,)", vec![(None, true), (None, true)]),
        (
            "f(1L,,3L)",
            vec![(None, false), (None, true), (None, false)],
        ),
        ("f(a=,b=2L)", vec![(Some("a"), true), (Some("b"), false)]),
        (
            "f(,,a,,b,,)",
            vec![
                (None, true),
                (None, true),
                (None, false),
                (None, true),
                (None, false),
                (None, true),
                (None, true),
            ],
        ),
        (
            "f(# before\n, # between\na= # named\n, # after\n)",
            vec![(None, true), (Some("a"), true), (None, true)],
        ),
        (
            "f(g(1L,2L), 'a,b',)",
            vec![(None, false), (None, false), (None, true)],
        ),
        (
            "f(`a b`=, 'quoted'=)",
            vec![(Some("`a b`"), true), (Some("'quoted'"), true)],
        ),
    ] {
        let file = parse(source);
        assert!(
            file.parse_errors.is_empty(),
            "{source}: {:?}",
            file.parse_errors
        );
        let [Stmt::Expr(Expr::Call { args, .. })] = file.stmts.as_slice() else {
            panic!("{source}: {:?}", file.stmts)
        };
        let actual: Vec<_> = args
            .iter()
            .map(|arg| (arg.name.as_deref(), matches!(arg.value, Expr::Missing(_))))
            .collect();
        assert_eq!(actual, expected, "{source}");
        for arg in args {
            assert!(arg.span.start <= arg.span.end && arg.span.end <= source.len());
            assert!(
                source.is_char_boundary(arg.span.start) && source.is_char_boundary(arg.span.end)
            );
        }
    }
}

#[test]
fn missing_subscripts_use_the_same_slots_without_duplicate_padding() {
    for (source, expected) in [
        ("x[]", vec![]),
        ("x[,j]", vec![true, false]),
        ("x[i,]", vec![false, true]),
        ("x[,,drop=FALSE]", vec![true, true, false]),
        ("x[,g('a,b'),]", vec![true, false, true]),
        ("x[[,]]", vec![true, true]),
    ] {
        let file = parse(source);
        let [Stmt::Expr(Expr::Index { args, .. })] = file.stmts.as_slice() else {
            panic!("{source}: {:?}", file.stmts)
        };
        assert_eq!(
            args.iter()
                .map(|arg| matches!(arg.value, Expr::Missing(_)))
                .collect::<Vec<_>>(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn unsupported_actual_values_are_not_missing() {
    let file = parse("f(a=1i,b=)");
    let [Stmt::Expr(Expr::Call { args, .. })] = file.stmts.as_slice() else {
        panic!("{:?}", file.stmts)
    };
    assert!(matches!(args[0].value, Expr::Unknown(_)));
    assert!(matches!(args[1].value, Expr::Missing(_)));
    assert_eq!(args[0].name.as_deref(), Some("a"));
    assert_eq!(args[1].name.as_deref(), Some("b"));
}
