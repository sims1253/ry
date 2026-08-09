# no-diag
# A user function's return type is inferred from its body and the call
# site receives a real type, so the assignment is well-typed.
addone <- function(x = 0) {
  x + 1L
}
y <- addone(5)
