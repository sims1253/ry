# oracle: must-pass
outer <- function() {
  Gen <- S7::new_class("Gen")
  inner <- function() { Gen <- 5L; Gen() }
  inner()
}
stopifnot(inherits(outer(), "Gen"))
callback <- function() 7L
Gen <- get("callback")
inner <- function() { Gen <- 5L; Gen() }
stopifnot(inner() == 7L)
parameter <- function(f = 1L) {
  nested <- function() { f <- 2L; f() }
  nested()
}
stopifnot(parameter(function() 9L) == 9L)
x <- function() 2L
stopifnot(identical(lapply(1:3, function(i) { x <- 1L; x() }), rep(list(2L), 3L)))
