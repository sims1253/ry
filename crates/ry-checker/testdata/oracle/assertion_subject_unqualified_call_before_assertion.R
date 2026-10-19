# oracle: must-pass
# A literal base message() before the assertion runs no project code here,
# so the assertion still proves `x` NULL or scalar for the later `||`.
f <- function(x) {
  message("checking")
  stopifnot(is.null(x) || length(x) == 1L)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(suppressMessages(f(1L)))
