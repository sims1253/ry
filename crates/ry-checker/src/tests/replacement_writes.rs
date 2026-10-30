use super::*;

/// The five replacement writes from #839, checked against GNU R 4.5.0.
/// Before the fix, `dump-facts` exported the pre-write type for each.
#[test]
fn indexed_replacement_carries_the_new_mode_and_length() {
    let source = "\
ch <- character(0); ch[[\"a\"]] <- \"A\"
nl <- NULL; nl[[1]] <- 1:3
x <- 1:3; x[5] <- 2.5
v <- 1:3; v[[2]] <- list(1)
";
    let (diagnostics, scope) = check_with_scope(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let ch = scope.get("ch").unwrap();
    assert_eq!(ch.mode, Mode::Character, "{ch:?}");
    assert_eq!(ch.length, Length::One, "{ch:?}");

    let nl = scope.get("nl").unwrap();
    assert_eq!(nl.mode, Mode::List, "{nl:?}");
    assert_eq!(nl.length, Length::One, "{nl:?}");

    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::Double, "{x:?}");
    assert_eq!(x.length, Length::Known(5), "{x:?}");

    let v = scope.get("v").unwrap();
    assert_eq!(v.mode, Mode::List, "{v:?}");
    assert_eq!(v.length, Length::Known(3), "{v:?}");
}

/// `for (i in 1:5) r[i] <- i`: the subscript is not a literal, so the
/// length is not provable, but the mode and a non-null result are.
#[test]
fn dynamic_subscript_widens_instead_of_keeping_the_old_length() {
    let (_, scope) = check_with_scope("r <- c()\nfor (i in 1:5) r[i] <- i\n");
    let r = scope.get("r").unwrap();
    assert_eq!(r.mode, Mode::Integer, "{r:?}");
    assert_eq!(r.length.exact(), None, "{r:?}");
}

#[test]
fn negative_and_zero_subscripts_never_extend() {
    for (source, length) in [
        ("x <- 1:3; x[-1] <- 9L", Length::Known(3)),
        ("x <- 1:3; x[-9] <- 9L", Length::Known(3)),
        ("x <- 1:3; x[0] <- 9L", Length::Known(3)),
        ("x <- 1:3; x[2] <- 9L", Length::Known(3)),
    ] {
        let (diagnostics, scope) = check_with_scope(source);
        assert!(diagnostics.is_empty(), "{source}: {diagnostics:?}");
        let x = scope.get("x").unwrap();
        assert_eq!(x.mode, Mode::Integer, "{source}: {x:?}");
        assert_eq!(x.length, length, "{source}: {x:?}");
    }
}

#[test]
fn null_base_takes_the_bracket_rule_for_its_kind() {
    let (_, scope) = check_with_scope("a <- NULL; a[[1]] <- 1:3");
    assert_eq!(scope.get("a").unwrap().mode, Mode::List);
    assert_eq!(scope.get("a").unwrap().length, Length::One);

    let (_, scope) = check_with_scope("b <- NULL; b[2] <- 1L");
    assert_eq!(scope.get("b").unwrap().mode, Mode::Integer);
    assert_eq!(scope.get("b").unwrap().length, Length::Known(2));

    let (_, scope) = check_with_scope("c <- NULL; c[[\"k\"]] <- 1:3");
    assert_eq!(scope.get("c").unwrap().mode, Mode::List);
    assert_eq!(scope.get("c").unwrap().length, Length::One);
}

/// #840: `[[<-` with a name on an empty character vector is valid R.
/// `$<-` on an atomic vector only warns and coerces the LHS to a list;
/// neither write may raise the read-side RY061.
#[test]
fn atomic_replacement_writes_do_not_report_ry061() {
    for source in [
        "ch <- character(0); ch[[\"a\"]] <- \"A\"",
        "ch <- character(0); ch[\"a\"] <- \"A\"",
        "ch <- character(0); ch$a <- \"A\"",
        "ch <- c(a = \"x\"); ch[[\"a\"]] <- \"A\"",
        "ch <- c(\"a\", \"b\"); ch[1] <- \"z\"",
    ] {
        let diagnostics = check(source);
        assert!(
            diagnostics.iter().all(|d| d.code != "RY061"),
            "{source}: {diagnostics:?}"
        );
    }
    // The read side still errors.
    let diagnostics = check("ch <- 1:3; ch$a");
    assert!(
        diagnostics.iter().any(|d| d.code == "RY061"),
        "{diagnostics:?}"
    );
}

#[test]
fn named_write_extends_names_and_repeats_do_not() {
    let (_, scope) = check_with_scope("x <- c(a = 1L); x[[\"b\"]] <- 2L");
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::Integer);
    assert_eq!(x.length, Length::Known(2));

    let (_, scope) = check_with_scope("x <- c(a = 1L); x[[\"a\"]] <- 2L");
    let x = scope.get("x").unwrap();
    assert_eq!(x.length.exact(), Some(1));
}

/// A positional write preserves the names it does not replace, so a later
/// named write to one of them must not count the name as new.
#[test]
fn positional_write_keeps_names_for_later_named_writes() {
    let (_, scope) = check_with_scope("x <- c(a = 1L); x[1] <- 2L; x[[\"a\"]] <- 3L");
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::Integer);
    assert_eq!(x.length.exact(), Some(1));
}

/// `$<- NULL` deletes the element instead of storing it. On an atomic LHS
/// R first coerces to a list, then removes the name (if any), so the
/// length can shrink and a missing name adds nothing.
#[test]
fn dollar_null_deletes_instead_of_extending() {
    let (_, scope) = check_with_scope("x <- 1:3; x$a <- NULL");
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::List);
    assert_eq!(x.length, Length::AtLeast(2), "R keeps 3; 2 covers the drop");

    let (_, scope) = check_with_scope("y <- c(a = 1L); y$a <- NULL");
    let y = scope.get("y").unwrap();
    assert_eq!(y.mode, Mode::List);
    assert_eq!(y.length, Length::Zero);

    let (_, scope) = check_with_scope("z <- c(a = 1L, b = 2L); z$a <- NULL");
    let z = scope.get("z").unwrap();
    assert_eq!(z.length, Length::One);
}

/// A value that may be NULL must not be treated as a definite deletion.
/// With the name present, `$<-` either deletes (length-1) or replaces
/// (length); with the name absent it either does nothing or adds one.
#[test]
fn maybe_null_dollar_write_widens_between_delete_and_replace() {
    let source = "\
flag <- sample(c(TRUE, FALSE), 1)
x <- c(a = 1L)
v <- if (flag) NULL else 2L
x$a <- v
";
    let (_, scope) = check_with_scope(source);
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::List, "{x:?}");
    assert_eq!(x.length, Length::Unknown, "{x:?}");

    let source = "\
flag <- sample(c(TRUE, FALSE), 1)
x <- c(a = 1L)
v <- if (flag) NULL else 2L
x$b <- v
";
    let (_, scope) = check_with_scope(source);
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::List, "{x:?}");
    assert_eq!(x.length, Length::Nonempty, "{x:?}");
}

/// A recorded element type must not survive a positional write that
/// changed the vector's mode: `x[[\"a\"]]` would otherwise report the
/// pre-write type of that element.
#[test]
fn mode_changing_positional_write_forgets_stale_element_types() {
    let (_, scope) = check_with_scope("x <- c(a = 1L); x[1] <- \"z\"; y <- x[[\"a\"]]");
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::Character, "{x:?}");
    assert_eq!(scope.get("y").unwrap().mode, Mode::Character);
}

/// A subscript beyond R's integer range makes the write an error, not a
/// vector that long; the length must not claim it.
#[test]
fn out_of_range_subscript_literal_widens() {
    let (_, scope) = check_with_scope("x <- 1:3; x[99999999999999] <- 1L");
    assert_eq!(scope.get("x").unwrap().length, Length::Unknown);
}

/// `x[[i, j]] <- v` is recursive-list semantics; the single-literal view
/// cannot prove the touched position.
#[test]
fn multi_argument_double_subscript_widens() {
    let (_, scope) = check_with_scope("x <- 1:3; x[[1, 2]] <- 1L");
    assert_eq!(scope.get("x").unwrap().length, Length::Unknown);
}

#[test]
fn rebound_replacement_widens_to_unknown() {
    let (_, scope) = check_with_scope("`[<-` <- function(x, i, value) NULL\nx <- 1:3\nx[2] <- 5L");
    assert!(scope.get("x").unwrap().mode.is_unknown());
}

/// A very negative literal subscript goes through the negative-literal
/// path without overflowing the `i64` negation, and never extends.
#[test]
fn large_negative_subscript_literal_is_harmless() {
    let (_, scope) = check_with_scope("x <- 1:3; x[-99999999999999] <- 1L");
    let x = scope.get("x").unwrap();
    assert_eq!(x.length, Length::Known(3), "{x:?}");
}

/// A named `[<-`/`[[<-` whose value may be NULL is an ordinary named
/// write on the branch that runs; the length must widen because the NULL
/// branch errors on an atomic vector instead of deleting.
#[test]
fn maybe_null_named_index_write_widens() {
    for write in ["x[[\"a\"]] <- v", "x[\"a\"] <- v"] {
        let source = format!(
            "flag <- sample(c(TRUE, FALSE), 1)\nx <- c(a = 1L)\nv <- if (flag) NULL else 2L\n{write}"
        );
        let (_, scope) = check_with_scope(&source);
        let x = scope.get("x").unwrap();
        assert!(x.mode.is_unknown(), "{write}: {x:?}");
        assert_eq!(x.length.exact(), None, "{write}: {x:?}");
    }
}

/// An `is.na()` repair keeps the shape but coerces the mode like any
/// other write, so a non-numeric repair must not leave the old mode.
#[test]
fn non_numeric_is_na_repair_coerces_mode() {
    let (_, scope) = check_with_scope("x <- c(1, NA); x[is.na(x)] <- \"none\"");
    let x = scope.get("x").unwrap();
    assert_eq!(x.mode, Mode::Character, "{x:?}");
    assert_eq!(x.length, Length::Known(2), "{x:?}");
}

/// A rebound `[[<-` may return anything, so the numeric path must look up
/// the operator that actually runs.
#[test]
fn rebound_double_bracket_replacement_widens_on_numeric_base() {
    let (_, scope) =
        check_with_scope("`[[<-` <- function(x, i, value) NULL\nx <- 1:3\nx[[2]] <- 1L");
    assert!(scope.get("x").unwrap().mode.is_unknown());
}

/// An opaque value may be NULL, which deletes the element instead of
/// storing it.
#[test]
fn opaque_value_may_delete() {
    let (_, scope) = check_with_scope("x <- NULL; x$a <- compute()");
    let x = scope.get("x").unwrap();
    assert!(x.mode.is_unknown(), "{x:?}");
    assert_eq!(x.length.exact(), None, "{x:?}");
}

/// A definite NULL written into a NULL base stays NULL, whatever the
/// subscript; R returns NULL rather than an empty list.
#[test]
fn null_base_with_null_value_stays_null() {
    for source in [
        "x <- NULL; x[[1]] <- NULL",
        "x <- NULL; x[1] <- NULL",
        "x <- NULL; x$a <- NULL",
    ] {
        let (_, scope) = check_with_scope(source);
        let x = scope.get("x").unwrap();
        assert_eq!(x.mode, Mode::Null, "{source}: {x:?}");
        assert_eq!(x.length, Length::Zero, "{source}: {x:?}");
    }
}

/// A one-element character vector names its element like the bare string;
/// a longer one may add several names the length rule cannot count.
#[test]
fn one_element_character_vector_subscript_names_the_write() {
    let (_, scope) = check_with_scope("x <- c(a = 1L); x[c(\"b\")] <- 2L");
    assert_eq!(scope.get("x").unwrap().length, Length::Known(2));

    let (_, scope) = check_with_scope("x <- c(a = 1L); x[c(\"a\")] <- 2L");
    assert_eq!(scope.get("x").unwrap().length.exact(), Some(1));

    let (_, scope) = check_with_scope("x <- c(a = 1L); x[c(\"a\", \"b\")] <- 2L");
    assert_eq!(scope.get("x").unwrap().length.exact(), None);
}

/// A named write on a list or data frame adds an element the schema does
/// not hold; the length must follow even though the classless atomic path
/// does not handle these bases.
#[test]
fn named_write_extends_a_list_length() {
    let (_, scope) = check_with_scope("l <- list(a = 1L); l[\"b\"] <- 2L");
    assert_eq!(scope.get("l").unwrap().length, Length::Known(2));

    let (_, scope) = check_with_scope("l <- list(a = 1L); l[\"a\"] <- 2L");
    assert_eq!(scope.get("l").unwrap().length.exact(), Some(1));
}

/// `x[i] <- NULL` on an atomic vector is an error, so no shape is
/// reachable; do not export the base mode as if the write succeeded.
#[test]
fn null_value_on_an_atomic_base_widens() {
    let (_, scope) = check_with_scope("x <- 1:3; x[1] <- NULL");
    assert!(scope.get("x").unwrap().mode.is_unknown());
}

/// A named `[[` subscript selects an array dimension, which the
/// single-literal view cannot prove; it must not pin the length.
#[test]
fn named_double_subscript_widens() {
    let (_, scope) = check_with_scope("x <- 1:3; x[[i = 2]] <- 1L");
    assert_eq!(scope.get("x").unwrap().length.exact(), None);
}

/// An atomic named write coerces every element, so the recorded types of
/// untouched names must follow the new mode.
#[test]
fn named_write_coercion_updates_existing_element_types() {
    let (_, scope) = check_with_scope("x <- c(a = 1L); x[[\"b\"]] <- \"s\"; y <- x[[\"a\"]]");
    assert_eq!(scope.get("x").unwrap().mode, Mode::Character);
    assert_eq!(scope.get("y").unwrap().mode, Mode::Character);
}

/// `$<-` and `[[<-` with a new name add a list or data-frame field, and a
/// NULL value removes one instead.
#[test]
fn dollar_and_double_bracket_move_list_lengths() {
    for write in ["l$b <- 2L", "l[[\"b\"]] <- 2L", "l[\"b\"] <- 2L"] {
        let (_, scope) = check_with_scope(&format!("l <- list(a = 1L); {write}"));
        assert_eq!(scope.get("l").unwrap().length, Length::Known(2), "{write}");
    }
    let (_, scope) = check_with_scope("l <- list(a = 1L); l$a <- NULL");
    assert_eq!(scope.get("l").unwrap().length, Length::Zero);
    let (_, scope) = check_with_scope("l <- list(a = 1L); l$b <- NULL");
    assert_eq!(scope.get("l").unwrap().length.exact(), Some(1));
    let (_, scope) = check_with_scope(
        "flag <- sample(c(TRUE, FALSE), 1)\nl <- list(a = 1L)\nv <- if (flag) NULL else 2L\nl$b <- v",
    );
    assert_eq!(scope.get("l").unwrap().length.exact(), None);
}

/// R dedups repeated names for plain lists and vectors, but not for data
/// frames or the empty name.
#[test]
fn duplicated_and_empty_names_follow_r_lengths() {
    let (_, scope) = check_with_scope("l <- list(a = 1L); l[c(\"b\", \"b\")] <- 2L");
    assert_eq!(scope.get("l").unwrap().length, Length::Known(2));

    let (_, scope) = check_with_scope("l <- list(a = 1L); l[c(\"\", \"\")] <- 2L");
    assert_eq!(scope.get("l").unwrap().length, Length::Known(3));

    let (_, scope) = check_with_scope("df <- data.frame(a = 1L); df[c(\"b\", \"b\")] <- 7");
    assert_eq!(scope.get("df").unwrap().length, Length::Known(3));

    // The rule is per name: non-empty names still dedup next to an empty
    // one, and the empty name adds once per occurrence.
    for subscripts in [r#"c("b", "b", "")"#, r#"c("b", "", "b")"#] {
        let (_, scope) = check_with_scope(&format!("l <- list(a = 1L); l[{subscripts}] <- 1:3"));
        assert_eq!(
            scope.get("l").unwrap().length,
            Length::Known(3),
            "{subscripts}"
        );
    }
}

/// An opaque value may be NULL, which would delete the field instead of
/// adding it; the length must widen.
#[test]
fn opaque_named_write_on_a_list_widens_the_length() {
    let (_, scope) = check_with_scope("l <- list(a = 1L); l$b <- compute()");
    assert_eq!(scope.get("l").unwrap().length.exact(), None);
}

/// A named write on a classed atomic vector (factor, Date) is still
/// handled: R adds the element under that name.
#[test]
fn classed_atomic_named_write_is_handled() {
    let (diagnostics, scope) = check_with_scope("x <- factor(\"a\"); x[\"b\"] <- \"c\"");
    assert!(
        diagnostics.iter().all(|d| d.code != "RY061"),
        "{diagnostics:?}"
    );
    let x = scope.get("x").unwrap();
    assert!(x.columns.is_some(), "{x:?}");
    assert_ne!(x.length.exact(), Some(0), "{x:?}");
}

/// An incomplete schema (after a positional write) makes name membership
/// unknown, so a named write must widen instead of keeping the exact
/// length in either direction.
#[test]
fn unknown_name_presence_widens_list_and_delete_lengths() {
    let (_, scope) = check_with_scope("l <- list(a = 1L, b = 2L); l[[1]] <- 10L; l$c <- 3L");
    assert_eq!(scope.get("l").unwrap().length.exact(), None);

    let (_, scope) = check_with_scope("l <- list(a = 1L, b = 2L); l[[1]] <- 10L; l$b <- NULL");
    assert_eq!(scope.get("l").unwrap().length.exact(), None);
}
