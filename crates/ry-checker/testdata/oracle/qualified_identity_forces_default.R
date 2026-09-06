# oracle: must-warn RY098
# oracle-claim: RY098
recursive <- function(x = x) base::identity(x)
named <- function(x = x) base:::identity(x = x)
wrapped <- function(x = base::identity(x)) x
late <- function(x = value) {
  base::identity(x)
  value <- 1L
}
for (f in list(recursive, named, wrapped, late)) {
  result <- tryCatch(f(), error = function(e) e)
  stopifnot(inherits(result, "error"))
}
