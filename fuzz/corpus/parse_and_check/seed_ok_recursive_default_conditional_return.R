# no-diag
# A conditional early return means the later force is not guaranteed.
f <- function(x = x, flag) {
  if (flag) return(1L)
  x
}
f(flag = TRUE)
