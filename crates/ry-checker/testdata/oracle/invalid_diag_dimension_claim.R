# oracle: must-pass
# oracle-claim: RY128
# A length-one numeric `x` is diag()'s matrix dimension, not a
# diagonal entry: a negative or NA scalar errors before any matrix
# exists, while the same value with an explicit `nrow=` is a
# one-element diagonal. GPArotation's Random.Start shipped the
# scalar shape with a documented domain including the failing
# sign (#320); the flagged shapes are evaluated from strings so
# this claim file itself checks clean.
dim_error <- function(text, fragment) {
  err <- tryCatch(eval(parse(text = text)), error = identity)
  inherits(err, "error") && grepl(fragment, conditionMessage(err), fixed = TRUE)
}
stopifnot(dim_error("diag(-1L)", "invalid 'nrow' value (< 0)"))
stopifnot(dim_error("diag(NA)", "invalid 'nrow' value (too large or NA)"))
stopifnot(dim_error("diag(nrow = -1L)", "invalid 'nrow' value (< 0)"))
stopifnot(dim_error("diag(1L, ncol = NA_integer_)", "invalid 'ncol' value (too large or NA)"))
stopifnot(dim_error("diag(1L, ncol = -2L)", "invalid 'ncol' value (< 0)"))
stopifnot(dim_error("d <- -1L\ndiag(d)", "invalid 'nrow' value (< 0)"))
# A lone `ncol` errors on the missing `nrow` first, whatever its
# value, so ry does not claim an invalid-`ncol` error there.
stopifnot(dim_error("diag(ncol = 3L)", "argument \"nrow\" is missing, with no default"))
stopifnot(dim_error("diag(ncol = -1L)", "argument \"nrow\" is missing, with no default"))
# Entries mode and vector diagonals are legitimate.
stopifnot(identical(diag(-1, nrow = 1L), matrix(-1, 1L, 1L)))
stopifnot(identical(diag(-1, ncol = 2L), matrix(c(-1, 0), nrow = 1L)))
stopifnot(identical(diag(c(-1, -2)), diag(c(-1, -2), nrow = 2L)))
stopifnot(identical(diag(0), matrix(numeric(), 0L, 0L)))
