# oracle: must-pass
# Named handlers bind by name, so the first unnamed argument of tryCatch()
# is `expr` even after `error =` (#720). The result is the expression's
# value, or the handler's return value, never the handler function.
n <- tryCatch(error = function(e) 0L, 1L)
stopifnot(identical(n + 1L, 2L))
ok <- tryCatch(error = function(...) FALSE, {
  is.character("topic")
})
stopifnot(identical(ok, TRUE))
m <- tryCatch(error = function(e) 0L, expr = stop("boom"))
stopifnot(identical(m + 1L, 1L))
