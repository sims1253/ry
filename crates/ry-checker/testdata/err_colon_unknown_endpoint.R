# expect: RY040
# `1:n` is an integer or double vector even when `n` is unknown: `:` is
# not S3 generic and only two factor endpoints build a factor. Adding a
# character string errors in R (#648).
f <- function(n) {
  x <- 1:n
  x + "a"
}
