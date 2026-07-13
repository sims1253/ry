# expect: RY098
dmt <- function(x, mean = rep(0, d), S, df = Inf) {
  if (df == Inf) return(dmnorm(x, mean, S))
  d <- ncol(S)
  dmnorm(x, mean, S)
}
