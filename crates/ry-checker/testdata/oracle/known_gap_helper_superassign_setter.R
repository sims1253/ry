# oracle: known-gap an ordinary `<<-` in a helper runs an active binding's setter that rewrites the caller's x; ry trusts the helper's outward-write summary and keeps the scalar fact (false negative only: a kept fact can only suppress RY032)
makeActiveBinding("sink", function(v) {
  if (!missing(v)) {
    for (fr in rev(sys.frames())) if (exists("target_frame", envir = fr, inherits = FALSE)) assign("x", 1:2, envir = fr)
  }
  1
}, environment())
bump <- function() sink <<- 1L
f <- function(x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0L && TRUE)
  bump()
  if (is.null(x) || x == 1L) TRUE else FALSE
}
f()
