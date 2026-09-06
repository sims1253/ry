# expect: RY098, RY098, RY098, RY098, RY098
and_default <- function(x = x) TRUE && x
or_default <- function(x = x) FALSE || x
nested <- function(x = x) TRUE && (FALSE || x)
qualified <- function(x = x) base::force(TRUE && x)
late <- function(x = value) {
  FALSE || x
  value <- TRUE
}
