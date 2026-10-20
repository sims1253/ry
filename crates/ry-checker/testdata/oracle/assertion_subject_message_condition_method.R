# oracle: must-warn RY032
# Even a literal message() reaches conditionMessage() through its default
# handler, so a project method for it may rebind the caller's `x`.
conditionMessage.simpleMessage <- function(c) {
  for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  "m\n"
}
f <- function(x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  message("hi")
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
