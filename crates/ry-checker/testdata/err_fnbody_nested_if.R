# expect: RY040
# The arithmetic error sits inside an `if` inside a named function
# body; body-walking must reach nested branches and fire RY040.
f <- function(flag) {
  if (flag) {
    x <- "hello" + 1
  }
}
