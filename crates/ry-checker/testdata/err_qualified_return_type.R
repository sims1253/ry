# expect: RY040, RY040
# The qualified return's argument joins the function's return type the
# same way the bare keyword's does: both closures return character -- the
# statement after the return is unreachable and contributes nothing -- so
# each call site's arithmetic is a mode mismatch. Before the arm learned
# the qualified shape, `qualified` returned integer (the trailing 1L) and
# its call site stayed quiet.
bare <- function() {
  return("hello")
  1L
}
qualified <- function() {
  base::return("hello")
  1L
}
y <- bare() + 1L
z <- qualified() + 1L
