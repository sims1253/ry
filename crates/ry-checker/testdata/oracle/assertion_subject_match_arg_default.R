# oracle: must-warn RY032
# Without choices, match.arg() evaluates its caller's formal default even
# after the formal was reassigned, so a literal actual does not make it inert.
# (Such a default is non-literal, which already keeps the assertion from
# proving anything; match.arg() is an unaudited call either way.)
f <- function(x = 1L, arg = c("a", { assign("x", 1:2); "b" })) {
  stopifnot(x > 0L && TRUE)
  arg <- "a"
  arg <- match.arg(arg)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
