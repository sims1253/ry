# oracle: must-warn RY032
# Conservative boundary: an unqualified call before the assertion may have
# installed an active binding for `x`, so the assertion proves nothing, even
# though message() is harmless here.
f <- function(x) {
  message("checking")
  stopifnot(is.null(x) || length(x) == 1L)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(suppressMessages(f(1L)))
