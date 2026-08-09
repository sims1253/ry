# no-diag
conditional <- function(x) if (FALSE) x else 1L
conditional()

unreachable <- function(x) {
  return(1L)
  x
}
unreachable()

maybe_return <- function(x, flag) {
  if (flag) return(1L)
  x
}
maybe_return(flag = TRUE)

short_circuit_and <- function(x) {
  FALSE && x
  1L
}
short_circuit_and()

short_circuit_or <- function(x) {
  TRUE || x
  1L
}
short_circuit_or()
