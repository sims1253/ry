## case: slot extraction on an integer vector is silent
# no-diag
x <- 1:3; x@.Data

## case: slot extraction on a factor is silent
# no-diag
x <- factor('a'); x@.Data

## case: dollar on an integer vector is an atomic-vector error
# expect: RY061
x <- 1:3; x$.Data

## case: attach opens the search path for later and nested reads
# expect: RY010, RY010
# Only `before_attach` and the attached `dataset` argument are unbound.
# Every read after `attach()` may come from the attached object.
before_attach
attach(dataset)
after_attach
f <- function() { nested_after_attach }
g <- function() {
  attach(local_data)
  local_after_attach
  inner <- function() nested_local_after_attach
}

## case: require of an unstubbed package opens the search path
# no-diag
require(unstubbed_package)
from_attached_package

## case: library of an unknown package opens the search path for later reads
# expect: RY010
# Only `before_library` is unbound. A dynamic `character.only` library
# call also opens the search path.
before_library
library(fakepkg123)
after_library
f <- function() nested_after_library
library(package_name, character.only = TRUE)
after_dynamic_library

## case: library of a stubbed package keeps unknown names unbound
# expect: RY010
# dplyr has a stub, so `still_not_a_dplyr_thing` stays unbound.
library(dplyr)
still_not_a_dplyr_thing

## case: source without local leaves function reads unbound
# expect: RY010
# `source()` evaluates into the global environment, not the function frame.
f <- function() {
source("generated.R")
genuinely_missing
}

## case: positional local TRUE sources into the function frame
# no-diag
f <- function() {
  source("generated.R", TRUE)
  generated_binding
}

## case: source exprs with local TRUE binds in the function frame
# no-diag
f <- function() {
  source(exprs = expression(generated_binding <- TRUE), local = TRUE)
  generated_binding
}

## case: named local before file sources into the function frame
# no-diag
f <- function() {
  source(local = TRUE, file = "generated.R")
  generated_binding
}

## case: partial name lo matches local
# no-diag
# R partial-matches `lo` to the `local` formal.
f <- function() {
  source(file = "generated.R", lo = TRUE)
  generated_binding
}

## case: unknown local environment opens the function scope
# no-diag
f <- function() {
  source("generated.R", local = unknown_environment())
  generated_binding
}

## case: positional local FALSE leaves function reads unbound
# expect: RY010
f <- function() {
  source("generated.R", FALSE)
  genuinely_missing
}

## case: named local FALSE leaves function reads unbound
# expect: RY010
f <- function() {
  source(file = "generated.R", local = FALSE)
  genuinely_missing
}

## case: sourceCpp opens the search path for later and nested reads
# expect: RY010
# Only `before_source` is unbound.
before_source
Rcpp::sourceCpp("generated.cpp")
after_source
f <- function() nested_after_source

## case: local function named attach has no attach scope effect
# expect: RY010, RY010
# `dataset` and `still_missing` stay unbound because the local `attach`
# is an ordinary function.
factory <- function() function(x) x
attach <- factory()
attach(dataset)
still_missing

## case: shiny testServer injects session input and output
# no-diag
library(shiny)
testServer(NULL, {
  session$setInputs(x = 1L)
  input$x
  output$value
})

## case: shiny reactive code arguments are quoted
# no-diag
library(shiny)
reactive(missing_reactive)
observe(missing_observe)
observeEvent(missing_event, missing_handler)
eventReactive(missing_event_reactive, missing_value_reactive)
isolate(missing_isolate)
renderText(missing_text)
renderPrint(missing_print)
renderUI(missing_ui)
renderPlot(missing_plot)
renderTable(missing_table)
renderDataTable(missing_data_table)
renderImage(missing_image)
testServer(NULL, missing_test_server)

## case: local function shadows a typed package constant
# no-diag
library(rlang)
na_chr <- function() "ok"
value <- na_chr()
f <- function(na_chr) na_chr()
f(function() "formal")

## case: unresolved destructuring operator quotes its operands
# no-diag
# Without zeallot attached, `%<-%` is an unknown infix, so `unbound` is
# not read.
c(unbound) %<-% make_value()

## case: embrace resolves a bound formal outside a data mask
# no-diag
f <- function(x) {{ x }}

## case: embrace resolves the formal in function scope, not the data mask
# no-diag
library(dplyr)
f <- function(df, value) mutate(df, out = {{ value }})
f(data.frame(value = 1L), 2L)

## case: embrace of an unbound symbol reports typo
# expect: RY010
f <- function(x) {{ typo }}

## case: data pronoun resolves a known column
# no-diag
library(dplyr)
mutate(data.frame(known = 1L), out = .data$known)

## case: data pronoun double bracket resolves a known column
# no-diag
library(dplyr)
mutate(data.frame(known = 1L), out = .data[["known"]])

## case: data pronoun with a missing column is an undefined column
# expect: RY060
library(dplyr)
mutate(data.frame(known = 1L), out = .data$missing)

## case: data pronoun on an opaque mask is silent
# no-diag
library(dplyr)
f <- function(df) mutate(df, out = .data$anything)

## case: env pronoun resolves an enclosing binding
# no-diag
library(dplyr)
f <- function(df, bound) mutate(df, out = .env$bound)
f(data.frame(bound = 1L), 2L)

## case: env pronoun double bracket resolves an enclosing binding
# no-diag
library(dplyr)
f <- function(df, bound) mutate(df, out = .env[["bound"]])

## case: env pronoun reports the unbound name
# expect: RY010
library(dplyr)
f <- function(df) mutate(df, out = .env$unbound)

## case: bare data pronoun inside a mask is silent
# no-diag
library(dplyr)
mutate(data.frame(x = 1L), out = .data)

## case: scalar index of a length union drops the source lengths
# no-diag
# `x` is length 2 or 3. `x[1L]` has the index's length, so the condition
# does not see the source lengths.
f <- function(p, choice) {
x <- if (p) c("a", "b") else c("a", "b", "c")
y <- x[1L]
if (y == "a") 1L
}

## case: variable index of a length union drops the source lengths
# no-diag
f <- function(p, choice) {
x <- if (p) c("a", "b") else c("a", "b", "c")
y <- x[choice]
if (y == "a") 1L
}

## case: multi-argument index of a length union drops the source lengths
# no-diag
f <- function(p, choice) {
x <- if (p) c("a", "b") else c("a", "b", "c")
y <- x[1L, drop = TRUE]
if (y == "a") 1L
}

## case: scalar index of an opaque-length vector drops the source length
# no-diag
f <- function(flag, choice) {
choices <- c(if (flag) "a", "b", "c")
if (choices[1L] == "a") 1L
}

## case: variable index of an opaque-length vector drops the source length
# no-diag
f <- function(flag, choice) {
choices <- c(if (flag) "a", "b", "c")
if (choices[choice] == "a") 1L
}

## case: multi-argument index of an opaque-length vector drops the source length
# no-diag
f <- function(flag, choice) {
choices <- c(if (flag) "a", "b", "c")
if (choices[1L, drop = TRUE] == "a") 1L
}

## case: unbound index on an opaque-length vector reports undefined_index
# expect: RY010
f <- function(flag) { x <- c(if (flag) 1L, 2L, 3L); x[undefined_index] }

## case: zero-length member of a condition union is a condition error
# expect: RY001
# The zero-length branch errors in R with "argument is of length zero",
# even though the TRUE branch is valid. A find-or-NULL helper produces
# this shape: `where == "path"` over `character | NULL`.
x <- if (runif(1) > 0.5) logical(0) else TRUE
if (x) print(1)

## case: list and logical condition union stays possibly valid
# no-diag
# A member that is not proven zero-length keeps the whole union silent.
x <- if (runif(1) > 0.5) list(TRUE) else TRUE
if (x) print(1)

## case: s4 method on a signature call dispatches
# no-diag
setClass("C", slots = c(value = "numeric"))
setMethod("labels", signature("C"), function(object) c(label = "ok"))
x <- new("C")
y <- labels(x)
y[["label"]]

## case: s4 method on a named signature dispatches
# no-diag
setClass("SpatExtent", slots = c(value = "numeric"))
setMethod("as.vector", signature(x = "SpatExtent"), function(x) c(xmin = 1))
x <- new("SpatExtent")
v <- as.vector(x)
v[["xmin"]]

## case: s4 slot access and assignment are silent for any slot
# no-diag
setClass("C", representation(value = "numeric"))
x <- new("C")
a <- x@value
b <- x@undeclared
x@value <- 1
x@undeclared <- 2

## case: named vector columns survive transposed data frame constructors
# no-diag
v <- c(alpha = 1, beta = 2)
a <- data.frame(t(v))
b <- as.data.frame(t(v))
a$alpha
b$beta

## case: unknown vector names do not invent a data frame schema
# no-diag
make_row <- function(v) data.frame(t(v))
row <- make_row(c(1, 2))
row$anything

## case: function reading its own binding before assignment reports it
# expect: RY010
f <- function() { h <- h[["pre"]] }

## case: top-level use before assignment stays silent
# no-diag
# R evaluates a script top to bottom, so `y <- x` errors at runtime.
# The checker treats every top-level assignment as known to avoid false
# positives.
y <- x
x <- 1L

## case: namespace-qualified value is not unbound
# no-diag
# ry does not model other packages' exports, so `pkg::name` stays opaque.
#
# The parser preserves the full `pkg::name` spelling in `Expr::Ident`.
# The checker must (a) suppress RY010 for these in value and
# statement position (we don't model other packages' exports), and
# (b) still resolve `pkg::fn(args)` calls by stripping the prefix
# for typeshed lookups.
# `x <- S7::class_any` -- the RHS is a cross-package value
# reference. We can't resolve S7's export table, so we treat
# it as opaque and stay silent (no RY010).
x <- S7::class_any

## case: namespace-qualified statement is not unbound
# no-diag
# Reexport files contain bare `rlang::set_names` statements.
rlang::set_names

## case: namespace-qualified backtick operator is not unbound
# no-diag
magrittr::`%>%`

## case: triple-colon value is not unbound
# no-diag
x <- stats:::internal_helper

## case: namespace-qualified call to an unstubbed package is silent
# no-diag
x <- tibble::tibble(a = 1L)

## case: backtick percent operator is not unbound
# no-diag
# A user or package infix operator that no scope or stub defines.
x <- `%+%`

## case: backtick builtin operator is not unbound
# no-diag
# Primitives such as `+` are passed as values, for example to `Reduce`.
x <- `+`

## case: backtick pipe operator is not unbound
# no-diag
x <- `%>%`

## case: nested local function shadows a project function signature
# no-diag
helper <- function(required) required

outer <- function() {
  helper <- function() 1L
  helper()
}
outer()

## case: local function shadows the base inherits signature
# no-diag
f <- function(topics) {
  inherits <- function(type) function(x) x$inherits_from(type)
  topics$apply(inherits("return"))
}

## case: local function shadows base eval
# no-diag
g <- function(quo) {
  eval <- function() .Call("x", quo)
  eval()
}

## case: plain unbound reference is reported
# expect: RY010
# Control for the search-path cases below. Constructs that can add
# arbitrary bindings (library(), data(), load(), attach()) turn off RY010
# for the rest of the scope. Without one, a real miss still fires.
x <- genuinely_unbound_name

## case: library of an unknown package hides later unbound reads
# no-diag
library(definitely_not_a_real_pkg_xyz)
x <- some_unbound_value

## case: dynamic character.only library hides later unbound reads
# no-diag
pkg <- "x"
library(pkg, character.only = TRUE)
y <- still_unbound

## case: data and load hide later unbound reads
# no-diag
data(some_dataset)
load("workspace.rda")
z <- unbound_after_load

## case: attach of a list hides later unbound reads
# no-diag
attach(list(a = 1))
w <- unbound_after_attach

## case: sibling testthat blocks do not share bindings
# no-diag
# A loop variable in one `test_that()` block does not shadow a call head
# in another block. The file-level `prob` still resolves.
prob <- function(x) sum(x)
test_that("block A", {
for (prob in list(1, 2)) NULL
})
test_that("block B", {
prob(1:2)
})

## case: same-block shadowing keeps the call error
# expect: RY070
# R errors "could not find function" because `shadowed` is a number at
# call time.
test_that("a", {
  shadowed <- 1
  shadowed(2)
})

## case: braced argument of an ordinary call writes through
# expect: RY040
# In R, `identity({x <- 1})` changes the outer `x`. Block isolation applies
# only to the testthat callees.
identity({ x <- 1L })
x + "s"

## case: project-local test_that keeps caller argument flow
# expect: RY040
# The project definition evaluates its braced argument as an ordinary
# promise in the caller, so `zz` survives the block.
test_that <- function(desc, code) code
test_that("a", {
  zz <- 1L
})
zz + "s"

## case: describe and it blocks isolate sibling bindings
# no-diag
# The file-level `inner` still resolves.
inner <- function(x) sum(x)
describe("d", {
it("one", {
for (inner in list(1, 2)) NULL
})
it("two", {
inner(1:2)
})
})

## case: test_that wrapper argument is ordinary caller code
# expect: RY040, RY040
# In R, a wrapper that forwards its own `code` formal forces the promise in
# the wrapper's caller. Its errors are reported and its bindings reach that
# caller.
check_case <- function(desc, code) test_that(desc, code)
check_case("a", { zz <- 1L })
zz + "s"
check_case("b", 1L + "s")

## case: named testthat block arguments isolate the body
# no-diag
f <- function(x) x
it(code = { f <- 1L }, description = "a")
f(2)

## case: delayedAssign does not read the promise expression
# expect: RY010
# dplyr and lubridate bind shared values lazily. R does not evaluate the
# promise, so `later` is not read before it exists. Only `not_bound` is
# unbound.
delayedAssign('shared_empty', later)
later <- numeric()
f <- function() shared_empty
g <- function() {
  delayedAssign('forced', 2)
  function() forced
}
h <- function() not_bound

## case: delayedAssign with named reordered arguments binds x
# no-diag
f <- function() {
  delayedAssign(value = later, x = 'held')
  held
}

## case: delayedAssign with named x and positional value binds x
# no-diag
f <- function() {
  delayedAssign(x = 'held', later)
  held
}

## case: qualified delayedAssign binds x
# no-diag
f <- function() {
  base::delayedAssign('held', later)
  held
}

## case: delayedAssign with partial val binds x
# no-diag
f <- function() {
  delayedAssign(val = later, x = 'held')
  held
}

## case: delayedAssign with eval.env binds x
# no-diag
f <- function() {
  delayedAssign('held', later, eval.env = environment())
  held
}

## case: delayedAssign does not bind the value string
# expect: RY010
# `wrong` is the value, not the target, so it stays unbound.
delayedAssign(value = 'wrong', x = 'held')
wrong

## case: delayedAssign with a repeated x binds nothing
# expect: RY010
# R rejects a repeated formal before binding anything, so `held` is unbound.
f <- function() {
  delayedAssign(x = 'held', value = 1, x = 'other')
  held
}

## case: project delayedAssign is an ordinary call
# expect: RY003, RY010
# The project definition binds nothing, so `unused` is unbound.
delayedAssign <- function(x, value) 1L
y <- delayedAssign('unused', 0)
if (y) TRUE
unused

## case: local block closures read later helpers
# expect: RY010
# From rlang and cli `standalone-sizes.R`. A closure in a `local()` block
# reads helpers the block assigns later, as a function body would. Only
# `not_bound` is unbound.
format_bytes <- local({
pretty <- function(style) {
style <- switch(style, a = helper_a, b = not_bound)
if (is.null(style)) helper_a else style
}
helper_a <- function(x) x
pretty
})

## case: local block in the caller frame lets closures read later helpers
# no-diag
f <- function() {
local({
pretty <- function() helper_a
helper_a <- function(x) x
}, environment())
}

## case: closures in a braced call argument read later bindings of the block
# no-diag
# From ape `ace.R` (issue #690). A closure's free names are looked up when it
# runs, so a block passed to `suppressWarnings()` or a `switch()` arm may bind
# them after the definition, as a function body may.
fit_in_block <- function(x) {
  suppressWarnings({
    loss <- function(s) sum((x - mu)^2) / s
    mu <- mean(x)
    loss(2)
  })
}
fit_in_switch <- function(method, x) {
  switch(method, reml = {
    loss <- function(s) sum((x - mu)^2) / s
    mu <- mean(x)
    loss(2)
  })
}

## case: closures in a braced call argument still flag names the block never binds
# expect: RY010
f <- function(x) {
  suppressWarnings({
    loss <- function(s) sum((x - not_bound)^2) / s
    mu <- mean(x)
    loss(2)
  })
}

## case: bindings made in both arms of an if value are visible afterwards
# no-diag
# From xfun `cache.R` (issue #694). R binds an assignment in an `if` arm
# in the enclosing frame wherever the `if` sits.
f <- function(code, suffix) {
  n <- if (suffix == "") {
    hits <- TRUE
    length(code)
  } else {
    hits <- endsWith(code, suffix)
    which.max(hits)
  }
  list(n, hits)
}

## case: an if ending a braced call argument binds after the call
# no-diag
# From dplyr `summarise.R` (issues #350 and #694).
f <- function(verb) {
  withCallingHandlers({
    if (verb == "summarise") {
      sizes <- NULL
    } else {
      sizes <- 1L
    }
  }, error = function(e) stop(e))
  sizes
}

## case: an if ending a bare block binds after the block
# no-diag
f <- function(verb) {
  {
    if (verb == "summarise") sizes <- NULL else sizes <- 1L
  }
  sizes
}

## case: a binding made in one arm of an if value is silent like the statement form
# no-diag
f <- function(flag) {
  n <- if (flag) { extra <- 1; 2 } else 3
  extra
}

## case: an if value joins the types its arms bind
# expect: RY040
f <- function(flag) {
  n <- if (flag) { label <- "a"; 1 } else { label <- "b"; 2 }
  label + n
}

## case: a name no arm of an if value binds stays unbound
# expect: RY010
# The arm binds `hidden` only in the closure's own frame.
f <- function(flag) {
  n <- if (flag) { helper <- function() hidden <- 1; 1 } else 2
  hidden
}

## case: an opaque parameter call head is not the same-named stats function
# no-diag
# `dt` may be a function argument; R's call lookup stops at it (#722).
f <- function(dt) dt()$x

## case: an opaque local call head is not the same-named base function
# no-diag
# Unknown call results may be functions: no `nchar` arity or return applies.
g <- function(make) {
  dt <- make()
  d <- dt()
  d$x
}
h <- function(make) {
  nchar <- make()
  nchar()$x
}

## case: a NULL-or-function local call head is not the same-named base function
# no-diag
# Only the closure arm can be called, and it takes no `x`.
h <- function(flag) {
  nchar <- if (flag) NULL else function() list(x = 1)
  nchar()$x
}

## case: a function-valued local call head is not the same-named base function
# expect: RY061
# A factory result is a closure, not a literal: it takes no `x` (no RY091),
# and its own character result has no `$`.
make_nchar <- function() function() "n"
f <- function() {
  nchar <- make_nchar()
  nchar()$x
}

## case: a closure capturing a definite callable is not the same-named base function
# no-diag
# R's lookup from the inner frame stops at the enclosing binding.
get_nchar <- function() function() "n"
f <- function(nchar) {
  g <- function() nchar()
  g()
}
wrapper <- function() {
  nchar <- get_nchar()
  inner <- function() nchar()
  inner()
}
opaque <- function(make) {
  nchar <- make()
  inner <- function() nchar()$x
  inner()
}

## case: a write that may escape an unproven block keeps the base function
# expect: RY091
# ry cannot prove this `test_that` runs its block in a fresh frame, so
# `nchar` is only possibly bound there; R calls base `nchar`.
library(testthat)
test_that("a", {
  nchar <- function() "n"
})
nchar()

## case: a known non-function local still calls the base function
# expect: RY061
# R skips the double `c` and calls base `c`, whose result has no `$`.
m <- function() {
  c <- 1
  c(2, 3)$x
}

## case: `$` on a base data frame partially matches a unique prefix
# no-diag
# A base data.frame uses list `$`, so `df$nv` is `df$nvalid` (#753).
df <- data.frame(nvalid = 1:3, total = c(2, 4, 6))
df$total / df$nv

## case: `$` on a base data frame with an ambiguous prefix is an undefined column
# expect: RY060
df <- data.frame(nvalid = 1:3, nvx = 1:3)
df$nv

## case: `$` prefix on a tibble is an undefined column
# expect: RY060, RY060
# `$.tbl_df` never partial-matches: it warns and returns NULL. A join
# keeps its left input's tibble class.
tb <- dplyr::group_by(data.frame(nvalid = 1:3, k = 1:3))
tb$nv
dplyr::left_join(tb, data.frame(k = 1:3, z = 4:6), by = "k")$nv

## case: a closure formal named c does not void a stopifnot scalar fact
# no-diag
# Only a binding a registration can see masks S4 collection (#780).
asserted <- function(kind = "warning") {
  stopifnot(is.null(kind) || length(kind) == 1L)
  if (is.null(kind) || kind == "ignore") return(NULL)
  kind
}
on_error <- function(x) tryCatch(x, error = function(c) FALSE)

## case: a project-defined destructuring operator binds its pattern names
# no-diag
# rlang's standalone-zeallot.R and ggdist define their own `%<-%`; like
# zeallot's, it assigns each pattern name in the caller's frame (#706).
`%<-%` <- function(lhs, value) {
  lhs <- substitute(lhs)
  env <- parent.frame()
  vars <- as.list(lhs[-1])
  for (i in seq_along(vars)) assign(as.character(vars[[i]]), value[[i]], envir = env)
  invisible(value)
}
f <- function() {
  found <- NULL
  c(found, other) %<-% list(TRUE, 2)
  if (found) other
}

## case: a project-defined destructuring operator leaves other names alone
# expect: RY001
`%<-%` <- function(lhs, value) {
  vars <- as.character(substitute(lhs)[-1])
  for (i in seq_along(vars)) assign(vars[[i]], value[[i]], envir = parent.frame())
}
f <- function() {
  found <- NULL
  stale <- NULL
  c(found) %<-% list(TRUE)
  if (stale) 1
}
