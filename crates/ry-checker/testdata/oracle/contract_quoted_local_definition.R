# oracle: must-pass
# A backticked local literal binds `f` and `id`, so the calls reach it.
f <- function(x) x + 1L
id <- function(x) x
h <- function() {
  `f` <- function(x) paste(x)
  `id` <- function(x) 1L
  c(f("a"), id("a") + 1L)
}
stopifnot(identical(h(), c("a", "2")))
