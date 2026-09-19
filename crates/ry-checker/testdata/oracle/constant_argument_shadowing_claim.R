# oracle: must-warn RY111
# oracle-claim: RY111
# A call argument that hardcodes TRUE/FALSE for a formal an enclosing
# function exposes under the identical name silently ignores the caller's
# value: haven @ f067fb2, R/labelled.R:111 shipped exactly this shape
# (median.haven_labelled hardcoded `na.rm = TRUE` behind its own
# `na.rm = TRUE, ...` formals), so
# `median(labelled(c(1:4, NA), c(a = 1)), na.rm = FALSE)` returned 2.5
# where base `median(c(1:4, NA), na.rm = FALSE)` returns NA -- the
# caller's explicit value is what gets dropped, whatever the default.
# The R assertions below pin the premise on plain base median: the
# hardcoded inner constant wins over the caller's supplied value, while
# forwarding the formal restores the caller's control.
hardcoded <- function(x, na.rm = FALSE) {
  median(x, na.rm = TRUE)
}
forwarded <- function(x, na.rm = FALSE) {
  median(x, na.rm = na.rm)
}
# The pinned haven method shape: default TRUE, hardcode TRUE -- the
# caller's explicit na.rm = FALSE is still silently ignored.
haven_shaped <- function(x, na.rm = TRUE) {
  median(x, na.rm = TRUE)
}
x <- c(1:4, NA)
stopifnot(is.na(median(x, na.rm = FALSE)), is.na(median(x)))
stopifnot(identical(median(x, na.rm = FALSE), median(x)))
stopifnot(identical(median(x, na.rm = TRUE), 2.5))
# The caller's `na.rm = FALSE` (and the default) are both silently
# ignored by the hardcoded site -- the value is 2.5 either way.
stopifnot(identical(hardcoded(x), 2.5))
stopifnot(identical(hardcoded(x, na.rm = FALSE), 2.5))
stopifnot(identical(haven_shaped(x, na.rm = FALSE), 2.5))
# Forwarding the formal is the fix: the caller's value flows through.
stopifnot(is.na(forwarded(x)), identical(forwarded(x, na.rm = TRUE), 2.5))
# The callee-formal premise: median really does declare na.rm (R 4.6.1,
# `formals(median)` is `function (x, na.rm = FALSE, ...)`), which is what
# makes the hardcoded literal land in the ignored binding rather than in
# forwarded dots.
stopifnot(identical(names(formals(median)), c("x", "na.rm", "...")))
# A partial tag binds the same formal through R's partial matching, so a
# `na.r = TRUE` hardcode has the identical runtime defect; RY111 stays
# silent there by design (the exact spelling is the precision gate).
partial <- function(x, na.rm = FALSE) {
  median(x, na.r = TRUE)
}
stopifnot(identical(partial(x), 2.5))
