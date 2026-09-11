# oracle: must-pass
`+.flag` <- function(a, b) {
  makeActiveBinding("e", function(value) "TRUE", globalenv())
  a
}
d <- base::structure(1L, class = "flag")
x <- d + 1L
e <- "hello"
stopifnot(if (e) TRUE else FALSE)
