# no-diag
check_support <- function(support = c(0, Inf)) {
  if (!isTRUE(is.numeric(support) && length(support) == 2L && support[1] < support[2])) {
    stop("bad support")
  }
  if (is.finite(support[1])) {
    support[1] + 1e-12
  } else if (is.finite(support[2])) {
    support[2] - 1e-12
  } else {
    0
  }
}
