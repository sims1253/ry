# no-diag
# Direct recursion: the fixpoint loop converges on the return type
# (integer) without infinite descent.
fact <- function(n = 1L) {
  if (n <= 1L) {
    return(1L)
  }
  n * fact(n - 1L)
}
y <- fact(5)
