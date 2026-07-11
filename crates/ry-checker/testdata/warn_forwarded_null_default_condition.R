# expect: RY001
.helper <- function(flag = NULL) {
  if (flag) 1L else 0L
}

run <- function(flag = NULL) {
  .helper(flag = flag)
}

run()
