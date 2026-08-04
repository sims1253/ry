# no-diag
# A return inside a statically selected block makes the later reference
# unreachable and therefore cannot guarantee forcing the default.
f <- function(x = x) {
  if (TRUE) {
    return(1L)
    x
  }
}
f()
