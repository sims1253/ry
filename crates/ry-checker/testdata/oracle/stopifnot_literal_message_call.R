# oracle: must-pass
# A literal message() runs no project code here (no conditionMessage
# method), so the scalar fact survives it; `appendLF` is a literal control.
f <- function(x = 1L) {
  stopifnot(x > 0L && TRUE)
  message("checked", appendLF = TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(suppressMessages(f())), identical(suppressMessages(f(2L)), FALSE))
vector_call <- tryCatch(f(1:2), error = function(e) deparse(conditionCall(e)))
stopifnot(identical(vector_call, "x > 0L && TRUE"))
