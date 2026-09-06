# expect: RY098, RY098, RY098, RY098
recursive <- function(x = x) base::identity(x)
named <- function(x = x) base:::identity(x = x)
wrapped <- function(x = base::identity(x)) x
late <- function(x = value) {
  base::identity(x)
  value <- 1L
}
