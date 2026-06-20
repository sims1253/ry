# no-diag
# The `:` sequence operator produces an integer vector from two integer
# endpoints. The loop variable narrows to integer<1>, so arithmetic on
# it inside the body is well-typed.
total <- 0L
for (i in 1:100) {
  total <- total + i
}
