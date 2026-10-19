# oracle: must-pass
# A helper body that looks inert still runs syntax functions: a project
# `if` mask replaces `cls` when `pure()` runs, so a later `cls("after")`
# no longer reaches the annotated definition. ry keeps no identity across
# any user-function call (tests/declarations.rs).
`if` <- function(cond, yes, no) do.call("assign", list("cls", function(x) x, envir = globalenv()))
cls <- function(x) { stopifnot(identical(class(x), "integer")); x }
pure <- function() if (TRUE) NULL
b <- tryCatch(cls("before"), error = function(e) "error")
pure()
a <- cls("after")
base::stopifnot(base::identical(b, "error"), base::identical(a, "after"))
