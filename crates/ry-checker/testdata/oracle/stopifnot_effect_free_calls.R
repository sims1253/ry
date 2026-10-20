# oracle: must-pass
# A project helper whose body reads only literals has no effect on its
# caller, and a literal message() runs no project code here, so the scalar
# fact survives both calls (#647).
noop <- function() NULL
f <- function(x = 1L) {
  stopifnot(x > 0L && TRUE)
  noop()
  message("checked")
  if (is.null(x) || x == 1L) TRUE else FALSE
}
stopifnot(isTRUE(suppressMessages(f())), identical(suppressMessages(f(2L)), FALSE))
vector_call <- tryCatch(f(1:2), error = function(e) deparse(conditionCall(e)))
stopifnot(identical(vector_call, "x > 0L && TRUE"))
