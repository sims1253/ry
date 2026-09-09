# oracle: must-pass
# The optional first element changes the source length. A scalar subscript
# still selects one element, as in rstan's example-model menu.
choose <- function(flag) {
  choices <- c(if (flag) "a", "b", "c")
  if (choices[1L] == "a") 1L else 2L
}
stopifnot(choose(TRUE) == 1L, choose(FALSE) == 2L)
