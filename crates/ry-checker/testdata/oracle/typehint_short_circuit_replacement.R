# oracle: must-pass
# The RHS of `&&` runs in the current frame: `swap3()` replaces `cls3` for
# the rest of the script, not only inside the condition.
cls3 <- function(x) {
  stopifnot(identical(class(x), "integer"))
  x
}
swap3 <- function() { cls3 <<- function(x) x; TRUE }
before <- tryCatch(cls3("before"), error = function(e) "error")
if (TRUE && swap3()) NULL
after <- cls3("after")
stopifnot(identical(before, "error"), identical(after, "after"))
