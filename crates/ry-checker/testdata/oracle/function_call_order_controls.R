# oracle: must-pass
mean <- 1L
stopifnot(mean(c(1, 2)) == 1.5)
mean <- function(x) x
stopifnot(mean(2L) == 2L)
deferred <- function() {
  x <- 1L
  x()
}
x <- function() 2L
stopifnot(deferred() == 2L)
