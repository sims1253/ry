# oracle: known-gap a caller's calling handler runs during a literal message() and rewrites x through sys.frames(); ry keeps the scalar fact
f <- function(x = 1L) {
  stopifnot(x > 0L && TRUE)
  message("hi")
  if (is.null(x) || x == 1L) TRUE else FALSE
}
withCallingHandlers(
  f(),
  message = function(m) {
    for (e in sys.frames()) if (exists("x", envir = e, inherits = FALSE)) assign("x", 1:2, envir = e)
    invokeRestart("muffleMessage")
  }
)
