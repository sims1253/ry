## case: a NULL return guard makes an optional callback callable
# no-diag
new_source <- function(src, handler = NULL) {
  if (is.null(handler)) {
    return(src)
  }
  handler(src)
}
new_source("x")

## case: a return guard that keeps the NULL path still reaches the NULL default
# expect: RY001
f <- function(x = NULL) {
  if (!is.null(x)) return(x)
  if (x > 0) 1 else 2
}
f()

## case: a not-missing guard excludes the omitted NULL default
# no-diag
g_missing <- function(groupOnX = NULL) {
  if (!missing(groupOnX)) {
    if (groupOnX) "x" else "y"
  }
}
g_missing()

## case: a not-missing return guard leaves the omitted NULL default
# expect: RY001
f <- function(x = NULL) {
  if (!missing(x)) return(1)
  if (x > 0) 1 else 2
}
f()

## case: every not-null conjunct narrows its NULL default
# no-diag
# The attached package leaves base resolution unproven, as in a package.
library(ggplot2)
g_and <- function(min = NULL, max = NULL) {
  if (!is.null(min) && !is.null(max)) {
    if (min > max) stop("min > max")
  }
}
g_and()

## case: a non-empty length guard excludes the omitted NULL default
# no-diag
g_length <- function(select = NULL, keep = NULL) {
  if (length(select) == 1) {
    if (grepl("contrib", select)) "contrib"
  }
  if (length(keep) > 0) {
    if (keep %in% c("a", "b")) "known"
  }
}
g_length()

## case: a for body over a NULL default never sees the NULL
# no-diag
g_loop <- function(args, quote_args = NULL) {
  for (arg in quote_args) {
    if (arg %in% names(args)) args[[arg]] <- NULL
  }
  args
}
g_loop(list())

## case: a not-missing guard keeps an earlier is.null fact
# expect: RY001
# `f(NULL)` reaches `if (x)` with a supplied NULL.
f <- function(x = NULL) {
  if (is.null(x)) {
    if (!missing(x)) {
      if (x) 1
    }
  }
}
f()
f(NULL)

## case: a length guard in a disjunction does not narrow the NULL default
# expect: RY001, RY001
f <- function(x = NULL, verbose = FALSE) {
  if (length(x) > 0 || verbose) {
    if (x) 1
  }
  if (length(x) == 1 || is.na(x)) {
    if (x) 1
  }
}
f()

## case: a boolean alias of a not-null test narrows the NULL default
# no-diag
g_alias <- function(size = NULL, label = NULL) {
  has_size <- !is.null(size)
  if (has_size) {
    if (!size %in% c("a", "b")) stop("unknown column")
  }
  no_label <- is.null(label)
  if (!no_label) {
    if (label %in% c("a", "b")) "known"
  }
}
g_alias()

## case: a boolean alias stops narrowing once either name is rebound
# expect: RY001, RY001
f <- function(x = NULL, y = NULL) {
  has_x <- !is.null(x)
  x <- NULL
  if (has_x) {
    if (x > 0) 1
  }
  has_y <- !is.null(y)
  has_y <- TRUE
  if (has_y) {
    if (y > 0) 1
  }
}
f()

## case: a boolean alias stops narrowing after any write it may depend on
# expect: RY001, RY001, RY001
# `ok` also tests `y`; `cb` may run `assign("has_x", TRUE, envir =
# parent.frame())`; a backticked target writes the same `x`.
f <- function(x = NULL, y = NULL, cb = function() NULL) {
  ok <- !is.null(x) && is.numeric(y)
  y <- NULL
  if (ok) {
    if (y > 0) 1
  }
  has_x <- !is.null(x)
  cb()
  if (has_x) {
    if (x > 0) 1
  }
  has_x <- !is.null(x)
  `x` <- NULL
  if (`has_x`) {
    if (x > 0) 1
  }
}
f()
