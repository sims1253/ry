# oracle: must-pass
# A literal message() signals a condition, and a caller's calling handler
# can rebind a name the signalling frame resolves: `cls("after")` reaches
# the replacement. Audited base calls therefore keep scalar facts but drop
# declaration identities (tests/declarations.rs).
run <- function() {
  cls <- function(x) { stopifnot(identical(class(x), "integer")); x }
  message("hi")
  cls("after")
}
after <- withCallingHandlers(run(), message = function(m) {
  for (e in sys.frames()) {
    if (exists("cls", envir = e, inherits = FALSE)) {
      assign("cls", function(x) x, envir = e)
    }
  }
  invokeRestart("muffleMessage")
})
stopifnot(identical(after, "after"))
