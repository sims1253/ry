# no-diag
# Ordinary lazy default: use follows the body-local assignment.
lattice <- function(x, ylim = range(Px)) {
  Px <- compute(x)
  plot(ylim)
}

# A default referencing another parameter is not a body-local dependency.
parameter_default <- function(x, y = x) {
  y
}

# Assignment and use in one statement index are intentionally inconclusive.
same_statement <- function(x = local) {
  local <- x
}

# An unforced promise is harmless.
never_used <- function(x = local) {
  local <- 1L
  2L
}

# Creating a closure does not force its captured parameter.
called_late <- function(x = local) {
  force_later <- function() x
  local <- 1L
  force_later()
}
