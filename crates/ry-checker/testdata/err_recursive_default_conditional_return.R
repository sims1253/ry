# expect: RY109
# A conditional early return means the later force is not guaranteed, so the
# function runs when the flag is set. The default itself can never evaluate,
# so RY109 warns; RY098 stays quiet without a guaranteed force.
f <- function(x = x, flag) {
  if (flag) return(1L)
  x
}
f(flag = TRUE)
