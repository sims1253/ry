# expect: RY109
# A return inside a statically selected block makes the later reference
# unreachable, so this runs. RY098 reports only guaranteed forcing; RY109
# still warns, because the default itself can never evaluate.
f <- function(x = x) {
  if (TRUE) {
    return(1L)
    x
  }
}
f()
