# expect: RY040, RY040
# base::invisible(...) gets the arm's bare-invisible treatment: the
# argument type joins the return type (character here; the trailing
# assignment contributes nothing), and the call does not mark the block
# unreachable -- unlike a return it does not exit. Before the arm
# learned the qualified shape, `qualified` had no return contribution
# at all and its call site stayed quiet.
bare <- function() {
  invisible("hello")
  a <- 1L
}
qualified <- function() {
  base::invisible("hello")
  a <- 1L
}
y <- bare() + 1L
z <- qualified() + 1L
