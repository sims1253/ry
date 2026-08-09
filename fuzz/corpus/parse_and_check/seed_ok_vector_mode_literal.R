# no-diag
truncate_zeros <- function(n = 10L) {
  out <- vector(mode = "numeric", length = n)
  out[which(out == 0)] <- .Machine$double.xmin
  out
}
