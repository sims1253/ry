# oracle: must-pass
# A call guarded by a positive `is.function()` test cannot fail with
# "could not find function": on the guarded path the value is a function,
# whatever its other callers pass (spatstat.explore R/roc.R,
# spatstat.univar R/densityBC.R).
scale_dof <- function(z, dof = length(z)) {
  if (is.null(dof)) dof <- length(z)
  if (is.function(dof)) dof <- dof(length(z))
  dof
}
fit_outer <- function(z) fit_middle(z)
fit_middle <- function(z, dof = Inf) scale_dof(z, dof = dof)
stopifnot(identical(fit_outer(1:3), Inf))
stopifnot(identical(scale_dof(1:3, NULL), 3L))
stopifnot(identical(scale_dof(1:3, function(n) n * 2), 6))

degrees_else <- function(z) {
  dof <- length(z)
  if (!is.function(dof)) dof else dof(length(z))
}
stopifnot(identical(degrees_else(1:3), 3L))
