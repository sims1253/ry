# oracle: must-warn RY032
# A callback reached through a formal, a string, a computed value, or a
# condition handler installs the vector; any such call drops the fact.
# Rebind `x` in the nearest calling frame that defines `target_frame`.
replace_x <- function() {
  for (frame in rev(sys.frames())) {
    if (exists("target_frame", envir = frame, inherits = FALSE)) {
      return(assign("x", c(1L, 2L), envir = frame))
    }
  }
}
g <- function(...) replace_x()
formal_callback <- function(cb, x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  lapply(1L, cb)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
string_callback <- function(x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  lapply(1L, "g")
  if (is.null(x) || x == 1L) TRUE else FALSE
}
computed_callback <- function(x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  lapply(1L, if (TRUE) g else g)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
handler <- function(e) replace_x()
formal_handler <- function(h, x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  tryCatch(stop("boom"), error = h)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
for (run in list(
  function() formal_callback(g),
  function() string_callback(),
  function() computed_callback(),
  function() formal_handler(handler)
)) {
  vector_error <- tryCatch(run(), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
