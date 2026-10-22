# oracle: must-warn RY001
# Each guard excludes the omitted-argument path, so the NULL default cannot
# reach the guarded condition (#692): code after `if (is.null(x)) return()`,
# the `!missing(x)` branch, the second `&&` conjunct, `length(x) == 1`
# (FALSE for NULL), and a `for` body over NULL (never runs). A guard that
# keeps the NULL path still reaches `if (NULL > 0)`, and ry still warns.
g_return <- function(locale = NULL) {
  if (is.null(locale)) return(NULL)
  if (locale %in% c("en", "de")) "known" else "other"
}
g_missing <- function(groupOnX = NULL) {
  if (!missing(groupOnX)) {
    if (groupOnX) "x" else "y"
  }
}
g_and <- function(min = NULL, max = NULL) {
  if (!is.null(min) && !is.null(max)) {
    if (min > max) stop("min > max")
  }
}
g_length <- function(select = NULL) {
  if (length(select) == 1) {
    if (grepl("contrib", select)) "contrib"
  }
}
g_loop <- function(args, quote_args = NULL) {
  for (arg in quote_args) {
    if (arg %in% names(args)) args[[arg]] <- NULL
  }
  args
}
stopifnot(is.null(g_return()), is.null(g_missing()), is.null(g_and()))
stopifnot(is.null(g_length()), identical(g_loop(list()), list()))
stopifnot(identical(g_return("en"), "known"), identical(g_missing(TRUE), "x"))
stopifnot(identical(g_length("contrib"), "contrib"))
stopifnot(identical(g_loop(list(a = 1, b = 2), "a"), list(b = 2)))

keeps_null <- function(x = NULL) {
  if (!is.null(x)) return(x)
  if (x > 0) 1 else 2
}
error <- tryCatch(keeps_null(), error = identity)
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
