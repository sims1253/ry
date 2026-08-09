# oracle: must-pass
# oracle-claim: RY103
# class() can return multiple strings, so using its equality vector as an if
# condition raises a length error for a multi-class object.
x <- structure(list(), class = c("first", "second"))
error <- tryCatch({
  if (class(x) == "first") 1L
  NULL
}, error = identity)
stopifnot(inherits(error, "error"))
