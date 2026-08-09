# no-diag
# if/else branches returning different types (character vs integer) join
# via the coercion ladder. The result is well-typed at the call site.
f <- function(x = TRUE) {
  if (x) "yes" else 0L
}
y <- f(FALSE)
