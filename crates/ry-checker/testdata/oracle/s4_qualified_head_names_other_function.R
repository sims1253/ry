# oracle: must-pass
# The project's `identity` generic is a new function; `base::identity` is not it.
invisible(setClass("A", contains = "numeric"))
invisible(setGeneric("identity", function(x) standardGeneric("identity")))
invisible(setMethod("identity", "A", function(x) "s4"))
stopifnot(identical(identity(new("A", 1)), "s4"))
stopifnot(identical(as.numeric(base::identity(new("A", 1)) + 1), 2))
