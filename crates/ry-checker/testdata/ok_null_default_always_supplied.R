# no-diag
.helper <- function(flag = NULL) {
  if (flag) 1L else 0L
}

run <- function(flag = TRUE) {
  .helper(flag = flag)
}

run()
