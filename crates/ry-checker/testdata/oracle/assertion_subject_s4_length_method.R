# oracle: must-warn RY032
# An S4 length method reports one for a longer vector, so the assertion
# proves nothing; a project defining it gets no scalar facts.
setClass("foo", contains = "numeric")
setMethod("length", "foo", function(x) 1L)
f <- function(x) {
  stopifnot(length(x) == 1L)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
vector_error <- tryCatch(f(new("foo", c(1, 2))), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
