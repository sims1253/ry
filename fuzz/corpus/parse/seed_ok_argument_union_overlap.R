# no-diag
average_branch <- function(flag) {
  value <- if (flag) 1L else "text"
  mean(value)
}
