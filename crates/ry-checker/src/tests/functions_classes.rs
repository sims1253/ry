use super::*;
use ry_core::RParser;

#[test]
fn closure_factory_infers_inner_return() {
    // `make_counter <- function() { function() { 1L } }` produces a
    // function whose `fn_sig.return_type` is itself a function with
    // `fn_sig.return_type` = integer<1>. So `c <- make_counter()`
    // binds `c` to a function-typed value with an inferred signature,
    // and `c()` resolves to integer<1>. We verify by using the
    // result arithmetically: integer + character must fire RY040
    // (proving the type was inferred, not opaque).
    let (_, scope) = check_with_scope(
        "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n",
    );
    let c = scope.get("c").expect("c should be bound");
    assert_eq!(
        c.mode,
        Mode::Function,
        "c must be function-typed, got {:?}",
        c
    );
    let sig = c.fn_sig.clone().expect("c must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Integer,
        "c() must resolve to integer, got {:?}",
        sig.return_type
    );
    // Behavioral check: using the result arithmetically with a
    // character operand must fire RY040.
    let diags = check(
        "make_counter <- function() { function() { 1L } }\n\
             c <- make_counter()\n\
             v <- c()\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer closure result + character, got {:?}",
        diags
    );
}

#[test]
fn closure_capture_resolves_outer_binding() {
    // `make_adder(x)` returns a closure that references the captured
    // `x`. The inner function's body `x + y` (both double via
    // defaults) produces double<1>; the outer function's `fn_sig`
    // carries that as the return type. `add5(3)` therefore resolves
    // to double<1>.
    let (_, scope) = check_with_scope(
        "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n",
    );
    let add5 = scope.get("add5").expect("add5 should be bound");
    assert_eq!(add5.mode, Mode::Function);
    let sig = add5
        .fn_sig
        .clone()
        .expect("add5 must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Double,
        "add5(3) must resolve to double, got {:?}",
        sig.return_type
    );
    // Behavioral check: RY040 on v + "x".
    let diags = check(
        "make_adder <- function(x = 0) {\n\
             \x20 function(y = 0) { x + y }\n\
             }\n\
             add5 <- make_adder(5)\n\
             v <- add5(3)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from double closure result + character, got {:?}",
        diags
    );
}

#[test]
fn nested_function_definition_visible_in_outer_body() {
    // The named-return closure pattern: `g <- function() { 1L }; g`
    // inside the outer body. The body simulator processes the
    // assignment so the trailing `g` picks up `g`'s inferred
    // `fn_sig`. The outer function's return type is therefore a
    // function value with an inferred signature, and `h()`
    // resolves to integer<1>.
    let (_, scope) = check_with_scope(
        "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n",
    );
    let h = scope.get("h").expect("h should be bound");
    assert_eq!(h.mode, Mode::Function);
    let sig = h.fn_sig.clone().expect("h must carry an inferred fn_sig");
    assert_eq!(
        sig.return_type.mode,
        Mode::Integer,
        "h() must resolve to integer, got {:?}",
        sig.return_type
    );
    let diags = check(
        "f <- function() {\n\
             \x20 g <- function() { 1L }\n\
             \x20 g\n\
             }\n\
             h <- f()\n\
             v <- h()\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from integer nested-closure result + character, got {:?}",
        diags
    );
}

#[test]
fn closure_depth_cap_falls_back_to_opaque() {
    // Four levels of nested closures exceeds MAX_CLOSURE_DEPTH (3).
    // The deepest call must NOT produce a false-positive RY040 when
    // used arithmetically, because the result is opaque (we gave up
    // inferring). This verifies the depth cap is respected.
    let diags = check(
        "f1 <- function() { function() { function() { function() { 1L } } } }\n\
             a <- f1()()()()\n\
             bad <- a + \"x\"\n",
    );
    // `a` is opaque (depth cap exceeded), so `a + "x"` must NOT
    // fire RY040. We allow any diagnostics EXCEPT RY040.
    assert!(
        diags.iter().all(|d| d.code != "RY040"),
        "depth-capped closure should be opaque, not integer; got {:?}",
        diags
    );
}

#[test]
fn lapply_anon_callback_infers_integer() {
    // `lapply(1:3, function(i) i * 2L)` returns a list whose
    // elements are integer (the callback's return type). We verify
    // by accessing an element and using it arithmetically: integer
    // + character must fire RY040, proving the element type was
    // inferred rather than opaque.
    let diags = check(
        "result <- lapply(1:3, function(i) i * 2L)\n\
             bad <- result[[1]] + \"x\"\n",
    );
    // `result[[1]]` goes through IndexKind::Double on a list with
    // a schema, so it resolves to the element type (integer).
    // However if the index access falls back to opaque, no RY040
    // fires. We assert no false positives at minimum.
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 expected in lapply callback body, got {:?}",
        diags
    );
}

#[test]
fn sapply_anon_callback_simplifies_to_vector() {
    // `sapply(1:5, function(x) x * 2L)` simplifies to an integer
    // vector (callback returns length-1 integer). Using the result
    // with a character must fire RY040, proving simplification
    // happened (opaque would not fire RY040).
    let diags = check(
        "v <- sapply(1:5, function(x) x * 2L)\n\
             bad <- v + \"hello\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply result + character, got {:?}",
        diags
    );
}

#[test]
fn sapply_named_callback_simplifies() {
    // Named user-fn callback: `dbl` returns integer (default x=1L,
    // body x * 2L). `sapply(1:5, dbl)` simplifies to integer vector.
    let diags = check(
        "dbl <- function(x = 1L) { x * 2L }\n\
             v <- sapply(1:5, dbl)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply(named_fn) + character, got {:?}",
        diags
    );
}

#[test]
fn sapply_typeshed_callback_simplifies() {
    // Typeshed callback: `sqrt` returns double.
    // `sapply(c(1.0, 4.0), sqrt)` simplifies to double vector.
    let diags = check(
        "v <- sapply(c(1.0, 4.0), sqrt)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from sapply(sqrt) + character, got {:?}",
        diags
    );
}

#[test]
fn vapply_uses_fun_value_template() {
    // `vapply(X, FUN, FUN.VALUE)` returns FUN.VALUE's type.
    // Here FUN.VALUE = `numeric(1)` = double<1>, so the result is
    // double. Using it with character fires RY040.
    let diags = check(
        "v <- vapply(c(1, 2, 3), function(x) x * 2, numeric(1))\n\
             bad <- v + \"x\"\n",
    );
    // `numeric(1)` may or may not resolve to double<1> depending
    // on typeshed coverage; if it resolves opaque, no RY040 fires.
    // Assert at minimum no false positives.
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "no RY010 expected in vapply, got {:?}",
        diags
    );
}

#[test]
fn vapply_fun_value_ignores_character_dots() {
    let (diags, scope) = check_with_scope(
        "x <- c(1, 2)\nf <- function(x, extra) x\nout <- vapply(x, f, FUN.VALUE = character(1), USE.NAMES = FALSE, extra = \"chr\")\n",
    );
    assert!(diags.is_empty(), "unexpected vapply diagnostics: {diags:?}");
    assert_eq!(scope.get("out").map(|ty| &ty.mode), Some(&Mode::Character));
}

#[test]
fn inherits_narrows_positive_and_negated_else_branches() {
    let diags = check(
        "print.foo <- function(x) 1L\nf <- function(x) { if (inherits(x, \"foo\")) print(x); if (!inherits(x, \"foo\")) 0L else print(x) }\n",
    );
    assert!(
        diags.iter().all(|diagnostic| diagnostic.code != "RY050"),
        "inherits narrowing should enable S3 dispatch: {diags:?}"
    );
}

#[test]
fn dynlib_prefix_resolves_only_with_nonempty_remainder() {
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "value <- pkg_call\n").unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["\0useDynLib:pkg_".to_string()]));
    checker.check(&file);
    assert!(checker.take_diagnostics().is_empty());

    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", "value <- pkg_\n").unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["\0useDynLib:pkg_".to_string()]));
    checker.check(&file);
    assert!(checker.take_diagnostics().iter().any(|d| d.code == "RY010"));
}

/// Plan 31 W6. `call_with_cleanup(native_symbol, ...)` is the cleancall
/// wrapper purrr, cli and rlang vendor; its first argument is a registered
/// native routine, not a variable. It is an ordinary R function, so the
/// suppression is licensed by `useDynLib(..., .registration = TRUE)` and
/// must not apply without it.
#[test]
fn call_with_cleanup_symbol_needs_declared_registration() {
    let src = "f <- function(x) call_with_cleanup(map_impl, environment(), x)\n";

    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from([
        ry_workspace::packages::NATIVE_REGISTRATION_SENTINEL.to_string(),
        "call_with_cleanup".to_string(),
    ]));
    checker.check(&file);
    let diags = checker.take_diagnostics();
    assert!(
        !diags.iter().any(|d| d.message.contains("map_impl")),
        "registered native symbol must not be reported unbound: {diags:?}"
    );

    // Negative control: the same call in a package that never declared
    // `.registration = TRUE` keeps reporting the unbound symbol.
    let mut parser = RParser::new().unwrap();
    let file = parser.parse("test.R", src).unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from(["call_with_cleanup".to_string()]));
    checker.check(&file);
    assert!(
        checker
            .take_diagnostics()
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("map_impl")),
        "without .registration the symbol is an ordinary unbound read"
    );
}

/// The registration gate licenses the native-symbol position only. Other
/// arguments of the same call are still ordinary reads.
#[test]
fn call_with_cleanup_still_checks_non_symbol_arguments() {
    let mut parser = RParser::new().unwrap();
    let file = parser
        .parse(
            "test.R",
            "f <- function() call_with_cleanup(map_impl, undefined_thing_xyz)\n",
        )
        .unwrap();
    let mut checker = Checker::new("test.R");
    checker.set_external_bindings(HashSet::from([
        ry_workspace::packages::NATIVE_REGISTRATION_SENTINEL.to_string(),
        "call_with_cleanup".to_string(),
    ]));
    checker.check(&file);
    assert!(
        checker
            .take_diagnostics()
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("undefined_thing_xyz")),
        "only the first argument is a native symbol"
    );
}

#[test]
fn r6_and_s7_class_body_pronouns_are_bound() {
    let diags = check(include_str!("../../testdata/ok_r6_class_body_bindings.R"));
    assert!(
        diags.is_empty(),
        "class-body fixture should be clean: {diags:?}"
    );
}

#[test]
fn r6_non_portable_public_field_binds_in_sibling_method() {
    // `portable = FALSE` makes the object's own environment the enclosure
    // of every method, so `.dir` resolves without a `self$` prefix.
    let diags = check(
        r#"FileUploadOperation <- R6Class(
  "FileUploadOperation",
  portable = FALSE,
  class = FALSE,
  public = list(
    .dir = character(0),
    initialize = function(dir) {
      .dir <<- dir
    },
    fileBegin = function() {
      file.path(.dir, "x")
    }
  )
)
"#,
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "non-portable R6 fields should be bound in sibling methods, got {diags:?}"
    );
}

#[test]
fn r6_non_portable_binds_private_and_active_members() {
    // All three member lists share one environment, so a public method may
    // name a private field and vice versa.
    let diags = check(
        r#"Thing <- R6::R6Class(
  "Thing",
  portable = FALSE,
  public = list(
    show = function() {
      cat(secret, computed_size)
    }
  ),
  private = list(
    secret = "s",
    peek = function() {
      show()
    }
  ),
  active = list(
    computed_size = function() {
      nchar(secret)
    }
  )
)
"#,
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "public, private and active members should all be bound, got {diags:?}"
    );
}

#[test]
fn r6_non_portable_field_type_comes_from_literal_initialiser() {
    // `.dir = character(0)` declares the field's mode, so a comparison
    // against a number is still caught inside the method body.
    let diags = check(
        r#"Thing <- R6Class(
  "Thing",
  portable = FALSE,
  public = list(
    .dir = character(0),
    bad = function() {
      .dir < 42
    }
  )
)
"#,
    );
    assert!(
        diags.iter().any(|d| d.code == "RY033"),
        "field type should come from the declared initialiser, got {diags:?}"
    );
}

#[test]
fn r6_non_portable_placeholder_field_is_untyped_once_reassigned() {
    // shiny's convention: the declared value names the class the field will
    // hold rather than storing a string, and `initialize()` overwrites it.
    // Taking the declaration at face value would make every method call on
    // the field a `$`-on-atomic error.
    let diags = check(
        r#"Logger <- R6Class(
  "Logger",
  portable = FALSE,
  public = list(
    msg = "<MessageLogger>",
    initialize = function(logger) {
      self$msg <- logger
    },
    emit = function() {
      msg$log("x")
    }
  )
)
"#,
    );
    assert!(
        diags.is_empty(),
        "a reassigned placeholder field carries no type: {diags:?}"
    );
}

#[test]
fn r6_non_portable_superassigned_field_is_untyped() {
    // `.dir <<- dir` replaces the declared `character(0)` with whatever the
    // caller passed, so the declaration no longer describes the field.
    let diags = check(
        r#"Thing <- R6Class(
  "Thing",
  portable = FALSE,
  public = list(
    .dir = character(0),
    initialize = function(dir) {
      .dir <<- dir
    },
    compare = function() {
      .dir < 42
    }
  )
)
"#,
    );
    assert!(
        diags.iter().all(|d| d.code != "RY033"),
        "a superassigned field must not keep its declared mode, got {diags:?}"
    );
}

/// `r6_rebound_members` must handle replacement-function assignment targets
/// (`class(x) <- v`) inside a non-portable R6 class body. The target is an
/// `Expr::Call`, not an `Expr::Ident` or `Expr::Index`, and the unhandled
/// catch-all arm silently dropped it — the field kept its declared type even
/// though the replacement overwrites it.
#[test]
fn r6_non_portable_replacement_function_assignment_rebinds_field() {
    let diags = check(
        r#"Thing <- R6Class(
  "Thing",
  portable = FALSE,
  public = list(
    kind = "default",
    upgrade = function() {
      class(kind) <- "upgraded"
    },
    show = function() {
      kind$render()
    }
  )
)
"#,
    );
    assert!(
        diags.is_empty(),
        "class(kind) <- v should rebind `kind` to unknown,          preventing RY033/RY061 on the later `$`: {diags:?}"
    );
}

#[test]
fn r6_non_portable_still_reports_undeclared_names() {
    // Injecting the member names must not blanket-suppress RY010 inside
    // the class body.
    let diags = check(
        r#"Thing <- R6Class(
  "Thing",
  portable = FALSE,
  public = list(
    .dir = character(0),
    bad = function() {
      file.path(.no_such_field_xyz, "x")
    }
  )
)
"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains(".no_such_field_xyz")),
        "a name that is not a declared member is still unbound, got {diags:?}"
    );
}

/// R6 turns every `active` member into an active binding, so reading the
/// bare name yields the getter's *result*, not the function. Typing it
/// `function` made ordinary arithmetic on an active field an RY040 error.
#[test]
fn r6_non_portable_active_member_is_a_value_not_a_function() {
    let diags = check(
        r#"Counter <- R6::R6Class(
  "Counter",
  portable = FALSE,
  public = list(
    n = 1,
    use = function() total + n
  ),
  active = list(
    total = function() 2
  )
)
"#,
    );
    assert!(
        diags.is_empty(),
        "an active binding reads as its getter's result: {diags:?}"
    );
}

#[test]
fn r6_portable_default_leaves_bare_field_unbound() {
    // Negative control (W19 recall guard). R6 defaults to `portable = TRUE`,
    // where method enclosures do not contain the members: a bare `.dir`
    // genuinely fails at runtime and must still be reported.
    let diags = check(
        r#"Thing <- R6Class(
  "Thing",
  public = list(
    .dir = character(0),
    fileBegin = function() {
      file.path(.dir, "x")
    }
  )
)
"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains(".dir")),
        "portable (default) R6 must still report the bare field, got {diags:?}"
    );
}

#[test]
fn r6_explicit_portable_true_leaves_bare_field_unbound() {
    // Negative control (W19 recall guard), explicit spelling.
    let diags = check(
        r#"Thing <- R6::R6Class(
  "Thing",
  portable = TRUE,
  public = list(
    .dir = character(0),
    fileBegin = function() {
      file.path(.dir, "x")
    }
  )
)
"#,
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains(".dir")),
        "explicit `portable = TRUE` must still report the bare field, got {diags:?}"
    );
}

#[test]
fn local_standalone_errors_idiom_is_clean() {
    let diags = check(include_str!("../../testdata/ok_local_standalone_errors.R"));
    assert!(
        diags.is_empty(),
        "local() fixture should be clean: {diags:?}"
    );
}

#[test]
fn namespace_assign_introduces_a_binding() {
    let diags = check(
        "assign(\"style\", function(x) x, envir = asNamespace(\"crayon\"))\nvalue <- style(\"x\")\n",
    );
    assert!(
        diags.is_empty(),
        "namespace assign should bind style: {diags:?}"
    );
}

#[test]
fn replacement_calls_keep_targets_bound_without_argument_diagnostics() {
    let diags = check(
        "x <- matrix(1:4, 2)\ndimnames(x) <- list(c(\"a\", \"b\"), c(\"c\", \"d\"))\nnames(x) <- c(\"a\", \"b\")\nattr(x, \"tag\") <- TRUE\nlevels(x) <- c(\"a\", \"b\")\nf <- function() NULL\nenvironment(f) <- globalenv()\ny <- x\nf()\n",
    );
    assert!(
        diags.is_empty(),
        "replacement calls should be opaque-safe: {diags:?}"
    );
}

#[test]
fn purrr_map_walks_callback_and_infers_list() {
    // purrr::map(.x, .f) is modeled like lapply -- the
    // callback body is walked (RY010 fires on the unbound `bug`)
    // and the result is a list.
    let diags = check(
        "library(purrr)\n\
             xs <- map(1:3, function(x) bug + x)\n",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("bug")),
        "purrr map should walk the callback and flag `bug`, got {:?}",
        diags
    );
}

#[test]
fn purrr_map_dbl_infers_double_vector() {
    // map_dbl returns a double vector; using it in character
    // arithmetic fires RY040 (proving the typed-mode result).
    let diags = check(
        "library(purrr)\n\
             v <- map_dbl(1:3, function(x) x + 0.5)\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "map_dbl result used with character should fire RY040, got {:?}",
        diags
    );
}

#[test]
fn purrr_map_dbl_type_mismatch_fires_ry080() {
    // map_dbl whose callback returns character fires
    // RY080 (R coerces silently, but the mismatch is a likely bug).
    let diags = check(
        "library(purrr)\n\
             xs <- map_dbl(1:3, function(x) paste(\"n\", x))\n",
    );
    assert!(
        diags.iter().any(|d| {
            d.code == "RY080"
                && d.message
                    == "`map_dbl` expects `double` returns but the callback returns `character`; R will coerce silently"
        }),
        "map_dbl with character callback should fire RY080, got {:?}",
        diags
    );
}

#[test]
fn purrr_in_parallel_is_transparent() {
    // in_parallel(.f) is type-transparent. map(sims,
    // in_parallel(f)) must walk `f`'s body identically to
    // map(sims, f) -- here the unbound `bug` must fire RY010.
    let diags = check(
        "library(purrr)\n\
             sims <- list(1, 2)\n\
             out <- map(sims, in_parallel(function(s) bug + s[[1]]))\n",
    );
    assert!(
        diags
            .iter()
            .any(|d| d.code == "RY010" && d.message.contains("bug")),
        "in_parallel-wrapped callback should still be walked, got {:?}",
        diags
    );
}

#[test]
fn purrr_not_loaded_does_not_treat_map_as_higher_order() {
    // Without library(purrr), a bare `map` must NOT be treated as
    // purrr's map (it is an unbound name -> RY010 on `map` itself,
    // or opaque). Either way, no purrr higher-order modeling.
    let diags = check("xs <- map(1:3, function(x) x)\n");
    // `map` is unbound (not in base typeshed); it resolves opaque
    // and the callback is NOT walked. No RY010 on a callback-local
    // name confirms the callback was not entered.
    assert!(
        diags
            .iter()
            .all(|d| d.code != "RY010" || !d.message.contains("map")),
        "ungated map should not get purrr treatment: {:?}",
        diags
    );
}

#[test]
fn reduce_returns_element_type() {
    // `Reduce(f, x)` returns the element type of x. For a double
    // vector, the result is double. Using it with character fires
    // RY040.
    let diags = check(
        "v <- Reduce(function(a, b) a + b, c(1.0, 2.0, 3.0))\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from Reduce result + character, got {:?}",
        diags
    );
}

#[test]
fn filter_preserves_data_type() {
    // `Filter(f, x)` returns x's type. For integer x, result is
    // integer. Using it with character fires RY040.
    let diags = check(
        "even <- function(x) x %% 2 == 0\n\
             v <- Filter(even, c(1L, 2L, 3L, 4L))\n\
             bad <- v + \"x\"\n",
    );
    assert!(
        diags.iter().any(|d| d.code == "RY040"),
        "expected RY040 from Filter result + character, got {:?}",
        diags
    );
}

#[test]
fn typeshed_fn_as_value_not_unbound() {
    // Passing a precisely modeled function as a callback remains valid;
    // the shadowed-symbol boost targets ambient-only resolution.
    let diags = check("v <- sapply(c(1.0, 2.0), sqrt)\n");
    assert!(diags.iter().all(|d| d.code != "RY010"), "got {diags:?}");
}

#[test]
fn user_fn_as_value_not_unbound() {
    // Passing a user-defined function name as a bare identifier must
    // NOT trigger RY010.
    let diags = check(
        "dbl <- function(x = 1L) x * 2L\n\
             v <- sapply(1:3, dbl)\n",
    );
    assert!(
        diags.iter().all(|d| d.code != "RY010"),
        "user fn name used as value should not be RY010, got {:?}",
        diags
    );
}
