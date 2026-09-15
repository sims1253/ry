# oracle: must-warn RY109
# oracle-claim: RY109
# A default that names its own formal can only resolve to the promise itself:
# triggering it errors ("promise already under evaluation: recursive default
# argument reference") while a supplied argument skips the default entirely.
# dtplyr shipped this shape (bffe46e, fixed in dbe32a6); the conditional use
# below is exactly why a forcing proof (RY098) is not required for RY109.
is_step <- function(x) FALSE
auto_copy <- function(x, y, copy = copy) {
  if (is_step(y)) y else paste(x, y, copy)
}
err <- tryCatch(auto_copy(1L, 2L), error = identity)
stopifnot(
  inherits(err, "error"),
  grepl("promise already under evaluation", conditionMessage(err))
)
stopifnot(identical(auto_copy(1L, 2L, copy = TRUE), "1 2 TRUE"))
