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
