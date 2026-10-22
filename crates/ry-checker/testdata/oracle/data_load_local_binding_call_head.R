# oracle: must-pass
# A local or parameter binding named `data` or `load` is the function the
# call reaches, not `utils::data` / `base::load`, so its result is not the
# character vector of dataset names (#717). Without a local binding,
# `data(mtcars)` still loads the dataset for later reads.
f <- function() {
  data <- function() list(nodes = 1)
  data()$nodes
}
g <- function() {
  load <- function(path) list(a = 1)
  load("x")$a
}
k <- function(data) data()$nodes
ctrl <- function() {
  data(mtcars)
  mtcars$mpg
}
stopifnot(identical(f(), 1))
stopifnot(identical(g(), 1))
stopifnot(identical(k(function() list(nodes = 1)), 1))
stopifnot(identical(ctrl(), datasets::mtcars$mpg))
