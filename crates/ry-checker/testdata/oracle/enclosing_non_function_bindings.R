# oracle: must-flag
outer <- function() {
  f <- 1L
  inner <- function() { f <- 2L; f() }
  inner()
}
outer()
