# oracle: must-warn RY001
# A local bound to a NULL test carries that test into `if (alias)` and
# `if (!alias)`, so the omitted NULL default cannot reach the guarded
# condition (#692). Rebinding the alias breaks the link: `if (y > 0)` then
# reads the NULL default, and ry still warns.
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
stopifnot(is.null(g_alias()), identical(g_alias("a", "b"), "known"))
error <- tryCatch(g_alias("c"), error = identity)
stopifnot(identical(conditionMessage(error), "unknown column"))

rebound <- function(y = NULL) {
  has_y <- !is.null(y)
  has_y <- TRUE
  if (has_y) {
    if (y > 0) 1
  }
}
error <- tryCatch(rebound(), error = identity)
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
