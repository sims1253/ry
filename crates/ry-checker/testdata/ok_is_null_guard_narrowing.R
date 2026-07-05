# no-diag
# `is.null` guards narrow across the branch: the non-null branch may
# call the value as a function.
f <- function(fun = NULL) {
  if (is.null(fun)) identity(1) else fun(1)
}
g <- function(fun = NULL) {
  if (!is.null(fun)) fun(1) else identity(1)
}
# A variable known to be NULL, called in the non-null branch.
h <- function() {
  x <- NULL
  if (is.null(x)) {
    1
  } else {
    x(2)
  }
}
