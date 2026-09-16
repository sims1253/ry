# oracle: must-warn RY001
# A `<<-` nested inside a closure whose intervening frame binds the name
# as a formal lands in that frame, never the definition scope (issue
# #374's collector prunes it): running the closures rebinds the formal,
# the file-level binding keeps its stale NULL, and the loop condition
# errors exactly as ry's RY001 claims. The uncalled-writer control at
# the bottom pins the flip side -- `<<-` skips the writing frame, so the
# file-level binding is the one updated there.
x <- NULL
outer <- function(x) {
  inner <- function() x <<- TRUE
  inner
}
outer(NULL)()
error <- tryCatch({ while (x) break }, error = identity)
stopifnot(inherits(error, "error"))
stopifnot(identical(conditionMessage(error), "argument is of length zero"))
stopifnot(identical(x, NULL))

y <- NULL
flat <- function(y) { y <<- TRUE }
flat(NULL)
stopifnot(identical(y, TRUE))
