# expect: RY098, RY098, RY098, RY098, RY098, RY098, RY098, RY098
recursive <- function(x = x) base::identity(x)
named <- function(x = x) base:::identity(x = x)
wrapped <- function(x = base::identity(x)) x
late <- function(x = value) {
  base::identity(x)
  value <- 1L
}
force_recursive <- function(x = x) base::force(x)
force_named <- function(x = x) base:::force(x = x)
force_wrapped <- function(x = base::force(x)) x
force_late <- function(x = value) {
  base::force(x)
  value <- 1L
}
