# oracle: must-warn RY125
# oracle-claim: RY125
# A function value has no `dim`, so `ncol()`/`nrow()`/`dim()` of one is
# always `NULL` -- the broom shape, where `ncol(data)` meant a data
# variable but `data` resolves to the `utils::data` function on the
# search path, so the count silently vanishes (including from a cli
# template, which evaluates `{...}` in the calling environment). The
# boundary lines show why the rule names only these three calls:
# `NROW()`/`NCOL()`/`length()` of a closure are 1, not `NULL`, and a
# classed function can carry a `dim` method, so classed values stay
# outside the claim.
stopifnot(is.function(utils::data))
stopifnot(is.null(ncol(utils::data)))
stopifnot(is.null(nrow(utils::data)))
stopifnot(is.null(dim(utils::data)))
stopifnot(is.null(ncol(function() 1L)))
stopifnot(identical(NROW(utils::data), 1L))
stopifnot(identical(NCOL(utils::data), 1L))
stopifnot(identical(length(utils::data), 1L))
dim.clsfn <- function(x) c(2L, 3L)
clsfn <- structure(function() 1L, class = "clsfn")
stopifnot(identical(dim(clsfn), c(2L, 3L)))
stopifnot(identical(ncol(clsfn), 3L))
cols <- ncol(data)
cols
