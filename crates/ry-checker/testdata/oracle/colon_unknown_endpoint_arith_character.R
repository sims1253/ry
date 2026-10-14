# oracle: must-flag
# `1:n` is a plain integer or double vector whatever `n` is: `:` is not
# S3 generic, and only two factor endpoints build a factor interaction.
# Adding a character string errors (non-numeric argument to binary
# operator) even though `n` is an unknown parameter (#648).
f <- function(n) {
  x <- 1:n
  x + "a"
}
f(3)
