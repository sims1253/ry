# oracle: must-pass
# Default promises run in the callee's frame and can change later bindings.
f <- function(x = {
  makeActiveBinding("e", function(value) "TRUE", environment())
  1L
}) {
  x
  e <- "hello"
  if (e) 1L
}
answer <- f()
stopifnot(identical(answer, 1L))
