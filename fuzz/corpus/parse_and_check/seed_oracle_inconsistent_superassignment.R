# oracle: known-gap local assignment on one branch leaves the outer binding NULL
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
