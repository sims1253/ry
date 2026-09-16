# oracle: known-gap a <<- on one branch now makes the binding unknown (issue #374),
# so ry is silent while R still errors: the untaken branch left the outer
# binding NULL, and NULL && TRUE errors with "invalid 'x' type"
state <- NULL
initialize <- function(enabled) {
  if (enabled) {
    state <<- TRUE
  } else {
    state <- FALSE
  }
}
initialize(FALSE)
state && TRUE
