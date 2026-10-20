# oracle: known-gap length(y) forces a caller's promise whose code rewrites x through sys.frames(); ry keeps the scalar fact
mut <- function() {
  for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  1L
}
g <- function(x = 1L, y) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  length(y)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
g(y = mut())
