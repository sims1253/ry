# oracle: must-warn RY032
# message() formats a non-literal payload with as.character(), which runs a
# project method. Formals after `...` match exactly, so `payload = obj` is
# forwarded payload too, not a control.
as.character.boom <- function(x, ...) {
  for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  "boom"
}
positional <- function(x = 1L, obj) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  message(obj)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
named <- function(x = 1L, obj) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  message(payload = obj)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
boom <- structure(1, class = "boom")
for (fn in list(positional, named)) {
  vector_error <- tryCatch(suppressMessages(fn(obj = boom)), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
