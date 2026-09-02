use super::*;

#[test]
fn dataset_resolves_mtcars() {
    // `mtcars` is in the typeshed's datasets table; using it must
    // not emit RY010 (unbound variable).
    let diags = check("df <- mtcars\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "expected no RY010 for mtcars, got {:?}",
        diags
    );
}

#[test]
fn dataset_resolves_iris() {
    let diags = check("df <- iris\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "expected no RY010 for iris, got {:?}",
        diags
    );
}

#[test]
fn s3_dispatch_known_method() {
    // `print.foo` is defined; calling `print(x)` on a "foo"-class
    // value dispatches to it. No RY050.
    let diags = check(
        "print.foo <- function(x, ...) { invisible(x) }\n\
             x <- structure(list(), class = \"foo\")\n\
             print(x)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "expected no RY050 when method is defined, got {:?}",
        diags
    );
}

#[test]
fn registered_unexported_s3_method_in_stub_satisfies_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("base.json"),
        r#"{
            "schema_version": "1",
            "package": "base",
            "version": "test",
            "functions": {},
            "s3_methods": [
                {"generic": "print", "class": "default", "params": ["x", "..."], "return": {"mode": "opaque", "length": "unknown"}}
            ]
        }"#,
    )
    .unwrap();
    let stubs = Arc::new(ry_typeshed::load_stub_dir(dir.path()).unwrap());
    let diagnostics = check_with(
        "x <- structure(list(), class = \"unexported\")\nprint(x)\n",
        |c| c.set_user_stubs(stubs),
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY050"),
        "a registered default method must satisfy S3 dispatch: {diagnostics:?}"
    );
}

#[test]
fn string_literal_assignment_binds_and_registers_s3_methods() {
    let (diags, scope) = check_with_scope(
        "\"x\" <- 1L\n\
         \"Math.foo\" <- function(x, ...) .Generic\n\
         \"print.foo\" <- function(x, ...) invisible(x)\n\
         obj <- structure(list(), class = \"foo\")\n\
         print(obj)\n\
         x\n",
    );
    assert_eq!(scope.get("x").map(|ty| ty.mode), Some(Mode::Integer));
    assert!(
        diags.iter().all(|d| d.code != "RY010" && d.code != "RY050"),
        "quoted assignment names must bind and carry S3 semantics: {diags:?}"
    );
}

#[test]
fn alist_quotes_arguments_and_returns_a_list() {
    let (diags, scope) = check_with_scope("rules <- alist(e1 = e2, x = undefined)\n");
    assert_eq!(scope.get("rules").map(|ty| ty.mode), Some(Mode::List));
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "alist arguments are unevaluated expressions: {diags:?}"
    );
}

#[test]
fn all_function_union_is_callable_with_intersection_argument_checks() {
    let src = "f <- if (flag) function(x = 1L) 1L else function(x = \"x\") \"x\"\n\
               ok <- f(1L)\n\
               bad <- f(list())\n";
    let (diags, scope) = check_with_scope(src);
    assert!(
        diags.iter().all(|d| d.code != "RY070"),
        "all-function union must be callable: {diags:?}"
    );
    assert!(
        diags.iter().any(|d| d.code == "RY092"),
        "a mismatch shared by every callable member must be reported: {diags:?}"
    );
    assert_eq!(scope.get("ok").map(|ty| ty.mode), Some(Mode::Union));
}

#[test]
fn null_function_union_remains_unguarded_call_error() {
    let diags = check("f <- if (flag) NULL else function() 1L\nf()\n");
    assert!(
        diags.iter().any(|d| d.code == "RY070"),
        "NULL/function unions are not callable without narrowing: {diags:?}"
    );
}

#[test]
fn s3_dispatch_missing_method() {
    // `Summary` is a known S3 generic because it has another method, but
    // no default method. Its missing class-specific method is flagged.
    let diags = check(
        "Summary.other <- function(...) 1L\n\
             x <- structure(list(), class = \"undefined\")\n\
             Summary(x)\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY050"),
        "expected RY050 for missing method, got {:?}",
        diags
    );
}

#[test]
fn s3_dispatch_walks_every_class_before_reporting_a_miss() {
    let diags = check(
        "print.b <- function(x, ...) invisible(x)\n\
         x <- list()\n\
         class(x) <- c(\"a\", \"b\")\n\
         print(x)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "the second class's method must satisfy dispatch: {diags:?}"
    );
}

#[test]
fn data_frame_ops_preserve_scalar_arithmetic_schema_and_stay_quiet() {
    let (diags, scope) = check_with_scope(
        "d <- data.frame(a = 1:10, b = 11:20)\n\
         divided <- d / 2\n\
         compared <- d == 1\n\
         negated <- -d\n",
    );
    assert!(
        diags
            .iter()
            .all(|d| !matches!(d.code, "RY040" | "RY030" | "RY020")),
        "data-frame Ops must not produce primitive type errors: {diags:?}"
    );
    let divided = scope.get("divided").expect("divided should be bound");
    assert!(divided.class.contains("data.frame"), "{divided:?}");
    assert_eq!(
        divided.columns.as_ref().map(|schema| schema.columns.len()),
        Some(2)
    );
    assert_eq!(scope.get("compared").map(|ty| ty.mode), Some(Mode::Opaque));
}

#[test]
fn user_ops_and_group_generics_suppress_primitive_errors() {
    let (diags, scope) = check_with_scope(
        "Ops.money <- function(e1, e2) list(amount = 1L)\n\
         Math.money <- function(x, ...) x\n\
         m1 <- structure(list(), class = \"money\")\n\
         m2 <- structure(list(), class = \"money\")\n\
         total <- m1 + m2\n\
         nullable <- m1 + NULL\n\
         magnitude <- abs(m1)\n",
    );
    assert!(
        diags.iter().all(|d| !matches!(d.code, "RY040" | "RY020")),
        "Ops/Math methods must satisfy dispatch: {diags:?}"
    );
    assert_eq!(scope.get("total").map(|ty| ty.mode), Some(Mode::Opaque));
    assert_eq!(scope.get("nullable").map(|ty| ty.mode), Some(Mode::Opaque));
    assert_eq!(scope.get("magnitude").map(|ty| ty.mode), Some(Mode::Opaque));
}

#[test]
fn s3_dispatch_in_package_default_method_satisfies_dispatch() {
    let diags = check(
        "update.default <- function(x, ...) x\n\
             x <- structure(list(), class = \"undefined\")\n\
             update(x)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "an in-package default method must satisfy S3 dispatch: {diags:?}"
    );
}

#[test]
fn s3_dispatch_no_class() {
    // `y` has no class attribute (a plain atomic vector). S3
    // dispatch has nothing to work on; RY050 must NOT fire.
    let diags = check(
        "y <- c(1, 2, 3)\n\
             print(y)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "expected no RY050 on a classless value, got {:?}",
        diags
    );
}

#[test]
fn structure_call_sets_class() {
    // `structure(list(), class = "foo")` must produce a type whose
    // class vector contains "foo". We exercise this through the
    // public `Checker` API by relying on the fact that a missing
    // `Summary.foo` method would emit RY050 only if the class was
    // actually attached.
    let diags = check(
        "Summary.other <- function(...) 1L\nx <- structure(list(), class = \"foo\")\nSummary(x)\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY050"),
        "expected RY050 proving class was attached, got {:?}",
        diags
    );
}

#[test]
fn mtcars_mpg_column_infers_double() {
    // `df$mpg` on `mtcars` must resolve to the column's type
    // (double<32>, not opaque). We assert the inferred type of `x`
    // directly via the test scope, and also exercise a behavioral
    // check: `x + 1L` is well-typed (double + integer) and produces
    // no RY040.
    let (_, scope) = check_with_scope("df <- mtcars\nx <- df$mpg\n");
    let x = scope.get("x").expect("x should be bound");
    assert_eq!(
        x.mode,
        Mode::Double,
        "df$mpg must infer double, got {:?}",
        x
    );
    assert_eq!(x.length, Length::Known(32), "mpg has 32 rows");
    let diags = check("df <- mtcars\nx <- df$mpg\ny <- x + 1L\n");
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "x + 1L should be valid (double + int), got {:?}",
        diags
    );
}

#[test]
fn mtcars_undefined_column_emits_ry060() {
    // `mtcars$nonexistent` must emit RY060 (undefined-column). The
    // message should name the offending column and list available
    // ones so the user can fix the typo. The available-columns
    // preview is taken from the schema in (BTreeMap-sorted) order;
    // we assert on a column that lands in the first 5.
    let diags = check("df <- mtcars\nbad <- df$nonexistent\n");
    let hit = diags
        .iter()
        .find(|d| d.code == "RY060")
        .expect("expected RY060 for nonexistent column");
    assert!(
        hit.message.contains("nonexistent"),
        "message should name the column: {}",
        hit.message
    );
    assert!(
        hit.message.contains("cyl"),
        "message should list an available column (cyl is in the first 5 alphabetically): {}",
        hit.message
    );
    // Sanity: the message also indicates abbreviation (mtcars has
    // 11 columns, more than the 5-column preview limit).
    assert!(
        hit.message.contains("..."),
        "message should abbreviate the list: {}",
        hit.message
    );
}

#[test]
fn list_named_args_become_schema() {
    // `list(a = 1L, b = "x")` builds a column schema from the named
    // args; `l$a` resolves to integer<1> and `l$b` to character<1>.
    let (_, scope) = check_with_scope("l <- list(a = 1L, b = \"x\")\nva <- l$a\nvb <- l$b\n");
    let va = scope.get("va").expect("va should be bound");
    assert_eq!(va.mode, Mode::Integer, "l$a must be integer");
    assert_eq!(va.length, Length::One, "l$a is a scalar");
    let vb = scope.get("vb").expect("vb should be bound");
    assert_eq!(vb.mode, Mode::Character, "l$b must be character");
    // And the list itself should carry the schema.
    let l = scope.get("l").expect("l should be bound");
    let schema = l.columns.clone().expect("l should carry a column schema");
    assert_eq!(schema.len(), 2, "schema should have 2 columns");
    assert_eq!(schema.names(), vec!["a", "b"]);
    // Accessing a missing column on a PLAIN list is silent: in R
    // `l$missing` returns NULL, so RY060 is scoped to data frames
    // Only data-frame misses fire RY060.
    let diags = check("l <- list(a = 1L)\nbad <- l$missing\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "plain-list `$` miss must not fire RY060, got {:?}",
        diags
    );
}

#[test]
fn list_dots_produces_an_incomplete_schema() {
    // A dots expansion may add any field at runtime. Consequently an absent
    // field is not known NULL (and a later use must not produce RY040).
    let (_, scope) = check_with_scope("x <- list(...)\n");
    let x = scope.get("x").expect("x should be bound");
    assert!(
        x.columns.as_ref().is_some_and(|schema| !schema.complete),
        "list(...) must retain an incomplete schema"
    );

    let diagnostics = check(
        "f <- function(...) {\n\
         argument <- list(...)\n\
         argument$cex + 1L\n\
         }\n",
    );
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "RY040"),
        "a field supplied through dots is not known NULL: {diagnostics:?}"
    );

    let (diagnostics, scope) = check_with_scope("x <- list(a = 1L)\nx$missing + 1L\n");
    let x = scope.get("x").expect("x should be bound");
    assert!(
        x.columns.as_ref().is_some_and(|schema| schema.complete),
        "enumerable list arguments must retain a complete schema"
    );
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "RY040"),
        "a genuinely missing field on an enumerable list must remain known NULL: {diagnostics:?}"
    );
}

#[test]
fn data_frame_constructor_attaches_class() {
    // `data.frame(x = c(1L, 2L, 3L), y = c("a","b","c"))` must:
    // * produce a value whose class is `["data.frame"]`
    // * carry a column schema with `x` and `y`
    // * coerce column lengths to the common max (3)
    // (We use `c(1L, 2L, 3L)` rather than `1L:3L` because the `:`
    // operator conservatively returns `Length::Unknown` for its
    // result; `c(...)` gives us a concrete length-3 vector to test
    // the recycling logic.)
    let (_, scope) =
        check_with_scope("df <- data.frame(x = c(1L, 2L, 3L), y = c(\"a\", \"b\", \"c\"))\n");
    let df = scope.get("df").expect("df should be bound");
    assert!(
        df.class.contains("data.frame"),
        "data.frame() must attach class data.frame, got class {:?}",
        df.class
    );
    let schema = df.columns.clone().expect("df should carry a column schema");
    assert_eq!(schema.len(), 2, "schema should have 2 columns");
    // Column `x` is integer recycled to length 3.
    let x = schema.get("x").expect("x column should exist");
    assert_eq!(x.mode, Mode::Integer);
    assert_eq!(x.length, Length::Known(3), "x recycled to length 3");
    // Column access resolves through the schema.
    let (_, scope2) = check_with_scope("df <- data.frame(x = c(1L, 2L, 3L))\nxv <- df$x\n");
    let xv = scope2.get("xv").expect("xv should be bound");
    assert_eq!(xv.mode, Mode::Integer);
    assert_eq!(xv.length, Length::Known(3));
    // `print(df)` dispatches to the typeshed's `print.data.frame`
    // method, so no RY050 fires (proves the class is real).
    let diags = check("df <- data.frame(x = c(1L, 2L, 3L))\nprint(df)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY050"),
        "print(df) should dispatch to print.data.frame, got {:?}",
        diags
    );
}

#[test]
fn df_double_bracket_string_resolves_column() {
    // `df[["col"]]` resolves via the schema just like `df$col`.
    let (_, scope) = check_with_scope("df <- iris\nsl <- df[[\"Sepal.Length\"]]\n");
    let sl = scope.get("sl").expect("sl should be bound");
    assert_eq!(sl.mode, Mode::Double);
    assert_eq!(sl.length, Length::Known(150));
    // Non-string-literal arg falls back to opaque (no RY060).
    let diags = check("df <- mtcars\nx <- df[[some_var]]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "non-literal [[ arg should not emit RY060, got {:?}",
        diags
    );
}

#[test]
fn df_single_bracket_returns_base_type() {
    // `df[1]` keeps the existing opaque behavior (no schema lookup,
    // no RY060). The base type is preserved.
    let (_, scope) = check_with_scope("df <- mtcars\nsub <- df[1]\n");
    let sub = scope.get("sub").expect("sub should be bound");
    assert_eq!(sub.mode, Mode::List, "df[1] preserves base mode");
    assert!(
        sub.class.contains("data.frame"),
        "df[1] preserves the data.frame class"
    );
    // Single bracket never emits RY060 even on a known schema.
    let diags = check("df <- mtcars\nsub <- df[\"nonexistent\"]\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "single-bracket must not emit RY060, got {:?}",
        diags
    );
}

#[test]
fn structure_preserves_list_column_schema() {
    // `structure(list(a = 1L), class = "foo")` keeps the list's
    // column schema while attaching the class.
    let (_, scope) = check_with_scope("x <- structure(list(a = 1L, b = \"y\"), class = \"foo\")\n");
    let x = scope.get("x").expect("x should be bound");
    assert!(x.class.contains("foo"), "class foo must be attached");
    let schema = x.columns.clone().expect("schema must be preserved");
    assert_eq!(schema.names(), vec!["a", "b"]);
    // Column access works through the new class.
    let (_, scope2) =
        check_with_scope("x <- structure(list(a = 1L), class = \"foo\")\nav <- x$a\n");
    let av = scope2.get("av").expect("av should be bound");
    assert_eq!(av.mode, Mode::Integer);
}

#[test]
fn nse_subset_resolves_columns() {
    // `subset(mtcars, cyl == 4)` evaluates `cyl == 4` in a scope
    // augmented with `mtcars`'s column schema. Without the NSE
    // handler, `cyl` would be reported as unbound (RY010). With it,
    // the expression is well-typed and produces no diagnostics.
    let (diags, scope) = check_with_scope("df <- mtcars\nsmall <- subset(df, cyl == 4)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "subset NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // The result type is the same data frame type as the first arg.
    let small = scope.get("small").expect("small should be bound");
    assert!(
        small.class.contains("data.frame"),
        "subset() must preserve the data.frame class, got class {:?}",
        small.class
    );
    // Column schema is preserved so downstream column access works.
    assert!(
        small.columns.is_some(),
        "subset() must preserve the column schema"
    );
}

#[test]
fn nse_with_evaluates_expression() {
    // `with(mtcars, sum(mpg))` evaluates `sum(mpg)` against a scope
    // where `mpg` is bound to the `mtcars` column type. Without the
    // NSE handler, `mpg` would trigger RY010 inside the `sum` call.
    let (diags, scope) = check_with_scope("df <- mtcars\ntotal <- with(df, sum(mpg))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "with NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `with` returns whatever the expression evaluates to. `sum`
    // dispatches against the typeshed to a length-1 numeric.
    let total = scope.get("total").expect("total should be bound");
    assert!(
        matches!(total.mode, Mode::Double | Mode::Integer),
        "with(df, sum(mpg)) must infer a numeric result type, got {:?}",
        total
    );
    assert_eq!(total.length, Length::One, "sum returns a scalar");
}

#[test]
fn nse_transform_handles_new_column() {
    // `transform(mtcars, x = mpg * 2)` evaluates `mpg * 2` against
    // an augmented scope. Without the NSE handler, `mpg` would
    // trigger RY010 inside the arithmetic expression.
    let (diags, scope) = check_with_scope("df <- mtcars\ndf2 <- transform(df, x = mpg * 2)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "transform NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `transform` returns a data frame; v1 keeps the original
    // schema (does not fold in the new column type).
    let df2 = scope.get("df2").expect("df2 should be bound");
    assert!(
        df2.class.contains("data.frame"),
        "transform() must preserve the data.frame class, got class {:?}",
        df2.class
    );
}

#[test]
fn nse_subset_preserves_enclosing_scope() {
    // The augmented scope is local to the NSE call: column names
    // must NOT leak back. After `subset(mtcars, cyl == 4)`, a
    // subsequent bare reference to `cyl` must STILL emit RY010.
    let diags = check("df <- mtcars\nsmall <- subset(df, cyl == 4)\nbad <- cyl\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "column bindings from NSE verbs must not leak into the enclosing scope, got {:?}",
        diags
    );
}

#[test]
fn nse_subset_no_schema_falls_through_silently() {
    // A data frame without a known column schema (here, an
    // opaque-typed user variable) cannot be augmented, so column
    // references inside the expression still emit RY010. The NSE
    // handler does not suppress diagnostics it cannot justify.
    let diags = check("df <- some_unknown_thing\nsmall <- subset(df, cyl == 4)\n");
    // `some_unknown_thing` itself is unbound (RY010), and `cyl`
    // inside the NSE expression is also unbound because `df` has no
    // schema to inject. Both are correct.
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "expected RY010 for unbound `cyl` when df has no schema, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_resolves_columns() {
    // `filter(df, mpg > 20)` is dplyr's row filter. Without the
    // NSE handler, `mpg` would be reported as unbound (RY010). The
    // handler injects the data frame's column schema so the
    // comparison is well-typed.
    let (diags, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr filter NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    // `filter` preserves the data frame type.
    let small = scope.get("small").expect("small should be bound");
    assert!(
        small.class.contains("data.frame"),
        "filter() must preserve the data.frame class, got class {:?}",
        small.class
    );
    assert!(
        small.columns.is_some(),
        "filter() must preserve the column schema"
    );
}

#[test]
fn nse_dplyr_mutate_resolves_columns() {
    // `mutate(df, kml = mpg * 0.425)` evaluates `mpg * 0.425`
    // against an augmented scope. Without the handler, `mpg` would
    // fire RY010.
    let (diags, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ndf2 <- mutate(df, kml = mpg * 0.425)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr mutate NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    let df2 = scope.get("df2").expect("df2 should be bound");
    assert!(
        df2.class.contains("data.frame"),
        "mutate() must preserve the data.frame class, got class {:?}",
        df2.class
    );
}

#[test]
fn nse_dplyr_summarise_returns_data_frame() {
    // `summarise(df, m = mean(mpg))` collapses to a single-row data
    // frame. The column reference `mpg` resolves via the augmented
    // scope. The result is a fresh data frame type with the named
    // summary outputs, not the input column schema.
    let (diags, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarise(df, m = mean(mpg))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr summarise NSE handler should suppress RY010 on column refs, got {:?}",
        diags
    );
    let s = scope.get("s").expect("s should be bound");
    assert!(
        s.class.contains("data.frame"),
        "summarise() must return a data.frame class, got class {:?}",
        s.class
    );
    let columns = s.columns.as_ref().expect("summarise output schema");
    assert!(
        columns.get("m").is_some(),
        "missing summary column: {:?}",
        s
    );
    assert!(
        columns.get("mpg").is_none(),
        "summarise() must not expose the input column schema, got {:?}",
        s
    );
}

#[test]
fn nse_dplyr_summarize_alias_matches_summarise() {
    // The American-English `summarize` is an alias for `summarise`
    // and must dispatch to the same handler. `hp` resolves against
    // the augmented scope; the result is a data frame.
    let (diags, scope) =
        check_with_scope("library(dplyr)\ndf <- mtcars\ns <- summarize(df, m = mean(hp))\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr summarize alias should suppress RY010 on column refs, got {:?}",
        diags
    );
    let s = scope.get("s").expect("s should be bound");
    assert!(
        s.class.contains("data.frame"),
        "summarize() must return a data.frame class, got class {:?}",
        s.class
    );
}

#[test]
fn nse_dplyr_pipe_chain_resolves_columns() {
    // `mtcars %>% filter(cyl == 4) %>% select(mpg, hp)` desugars
    // to nested calls. Each stage's data frame is the previous
    // stage's result (mtcars for the first), so column references
    // resolve via the augmented scope and no RY010 fires.
    let src = "library(magrittr)\n\
         library(dplyr)\n\
         result <- mtcars %>% filter(cyl == 4) %>% select(mpg, hp)\n";
    let (diags, scope) = check_with_scope(src);
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "piped dplyr chain should suppress RY010 on column refs, got {:?}",
        diags
    );
    // The chain's final result is a data frame (select preserves
    // the type of its input, which here is `filter`'s output =
    // mtcars' type).
    let result = scope.get("result").expect("result should be bound");
    assert!(
        result.class.contains("data.frame"),
        "piped dplyr chain must preserve the data.frame class, got class {:?}",
        result.class
    );
}

#[test]
fn nse_dplyr_filter_non_dataframe_falls_through() {
    // `filter` is only treated as dplyr's verb when the first arg
    // looks like a data frame (has a column schema or the
    // `data.frame` class). Here the first arg is a bare integer;
    // the call should NOT be intercepted as NSE - the bare column
    // reference `mpg` (which is unbound here) should fire RY010
    // through the regular arg-inference path.
    let diags = check("x <- 1L\nr <- filter(x, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "filter() with a non-data-frame first arg should fall through and emit RY010 on `mpg`, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_ungated_falls_through_when_not_loaded() {
    // Package gating: a bare `filter(df, ...)` in a script that
    // has NOT loaded dplyr must NOT be treated as dplyr's verb.
    // The column reference `mpg` is genuinely unbound in this scope
    // (no library(dplyr)), so RY010 must fire.
    let diags = check("df <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "ungated filter() without library(dplyr) should fall through and emit RY010 on `mpg`, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_qualified_resolves_without_library() {
    // Package gating: `dplyr::filter(...)` is always treated as
    // dplyr's verb regardless of whether dplyr is loaded, because
    // the `dplyr::` prefix is an explicit namespace reference. So
    // the column ref `mpg` must NOT fire RY010.
    let diags = check("df <- mtcars\nsmall <- dplyr::filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "dplyr::-qualified filter() should suppress RY010 on column refs without library(dplyr), got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_library_records_loaded() {
    // Package gating: `library(dplyr)` records dplyr into the
    // loaded set, so a subsequent `filter(df, ...)` resolves as
    // dplyr's verb and the column ref `mpg` does NOT fire RY010.
    let diags = check("library(dplyr)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library(dplyr) + filter() should suppress RY010 on column refs, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_requirenamespace_does_not_attach_dplyr() {
    // `requireNamespace("dplyr")` permits qualified access but does not
    // attach dplyr, so an unqualified filter call keeps base semantics.
    let diags = check("requireNamespace(\"dplyr\")\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().any(|d| d.code == "RY010"),
        "requireNamespace(\"dplyr\") must not attach unqualified dplyr names, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_filter_tidyverse_counts_as_dplyr() {
    // `library(tidyverse)` loads dplyr transitively; the gating
    // treats tidyverse as a synonym for dplyr.
    let diags = check("library(tidyverse)\ndf <- mtcars\nsmall <- filter(df, mpg > 20)\n");
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "library(tidyverse) + filter() should suppress RY010 on column refs, got {:?}",
        diags
    );
}

#[test]
fn nse_dplyr_arrange_groupby_preserve_type() {
    // `arrange` and `group_by` walk their column-reference args in
    // the augmented scope and preserve the input data frame type.
    let src = "library(dplyr)\n\
         df <- mtcars\n\
         sorted <- arrange(df, mpg)\n\
         grouped <- group_by(df, cyl)\n";
    let (diags, scope) = check_with_scope(src);
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "arrange/group_by NSE handlers should suppress RY010 on column refs, got {:?}",
        diags
    );
    let sorted = scope.get("sorted").expect("sorted should be bound");
    assert!(
        sorted.class.contains("data.frame"),
        "arrange() must preserve the data.frame class, got class {:?}",
        sorted.class
    );
    let grouped = scope.get("grouped").expect("grouped should be bound");
    assert!(
        grouped.class.contains("data.frame"),
        "group_by() must preserve the data.frame class, got class {:?}",
        grouped.class
    );
}

#[test]
fn lapply_list_arith_does_not_fire_ry040() {
    // Iterating a list yields the unwrapped element,
    // so arithmetic inside the callback must not fire RY040.
    let src = "out <- lapply(list(1, 2, 3), function(x) x * 2)\n";
    let diags = check(src);
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "lapply over a homogeneous list must not fire RY040, got {:?}",
        diags
    );
}

#[test]
fn dollar_missing_on_plain_list_does_not_fire_ry060() {
    // `$` on a plain list with a missing name returns
    // NULL in R; RY060 must only fire for data frames.
    let diags = check("v <- list(a = 1, b = 2)$missing\n");
    assert!(
        diags.iter().all(|d| d.code != "RY060"),
        "`$` miss on a plain list must not fire RY060, got {:?}",
        diags
    );
}

#[test]
fn dollar_missing_on_plain_list_returns_null() {
    // The returned value matches R's NULL (not unknown).
    let (_, scope) = check_with_scope("v <- list(a = 1, b = 2)$missing\n");
    let v = scope.get("v").expect("v should be bound");
    assert!(
        matches!(v.mode, Mode::Null),
        "plain-list `$` miss should return NULL, got {:?}",
        v
    );
    assert!(
        matches!(v.length, Length::Zero),
        "NULL length should be Zero, got {:?}",
        v
    );
}

#[test]
fn arithmetic_with_known_null_reports_ry040() {
    for source in [
        "function(a) a / NULL\n",
        "res <- list()\nres$ns <- a\nres$np <- res$ns / res$nv\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().any(|diagnostic| diagnostic.code == "RY040"),
            "known-NULL arithmetic must report RY040, got {diags:?} for {source:?}"
        );
    }
}

#[test]
fn known_null_arithmetic_ignores_parameter_defaults_and_imported_schemas() {
    for source in [
        "f <- function(weights = NULL) 1 + weights\n",
        "df <- mtcars\nx <- df$not_a_real_column\ny <- x + 1L\n",
    ] {
        let diags = check(source);
        assert!(
            diags.iter().all(|diagnostic| diagnostic.code != "RY040"),
            "only literal NULL and locally-built-list schema misses may report RY040: {diags:?} for {source:?}"
        );
    }
}

#[test]
fn for_over_homogeneous_list_does_not_fire_ry040() {
    // `for (el in list(1, 2, 3))` binds `el` to the unwrapped element
    // (double<1>) inside the loop body, so accumulating into `total`
    // is well-typed. (The loop var lives in the loop's child scope,
    // so we assert on the absence of RY040, not on `el`'s binding.)
    let diags =
        check_with_scope("total <- 0\nfor (el in list(1, 2, 3)) { total <- total + el }\n").0;
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "for over a homogeneous list must not fire RY040 on the body, got {:?}",
        diags
    );
}
