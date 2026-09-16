# oracle: known-gap RY109 defusing credit does not track a later evaluation of the captured promise: R errors, ry stays silent
# The capture is real (enquo receives the unevaluated default), but
# eval_tidy() later forces it, so R errors with "promise already under
# evaluation". ry credits the defuse and does not warn. The base twin
# (substitute + eval) behaves the same in R.
suppressMessages(library(rlang))
f <- function(x = x) {
  q <- rlang::enquo(x)
  rlang::eval_tidy(q)
}
f()
