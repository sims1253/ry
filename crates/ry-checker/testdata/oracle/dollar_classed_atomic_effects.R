# oracle: must-pass
# The method can write to the caller even when the payload and field are literal.
`$.ry_dollar_effects` <- function(x, name) {
  assign("marker", 1L, envir = parent.frame())
  2L
}
read_field <- function() {
  marker <- "before"
  x <- structure(1L, class = "ry_dollar_effects")
  result <- x$field
  stopifnot(identical(result, 2L), identical(marker + 1L, 2L))
}
read_field()
