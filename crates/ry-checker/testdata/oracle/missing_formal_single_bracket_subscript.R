# oracle: must-warn RY091
# A missing formal passed bare to `[`, positionally or as `drop = d`, is the
# empty subscript, and `x[i] <- v` with `i` omitted assigns every element:
# each of these returns normally. `[[` has no empty subscript and `x[-i]`
# evaluates `i`, so `get_elem` and `drop_neg` error.
get_cells <- function(x, i, j) {
  x[i, j, drop = FALSE]
}
stamp <- function(n, pages) {
  pages <- seq_len(n)[pages]
  pages
}
first_row <- function(m, d) m[1, , drop = d]
zero_at <- function(x, i) {
  x[i] <- 0
  x
}
get_elem <- function(x, i) {
  x[[i]]
}
drop_neg <- function(x, i) x[-i]
m <- matrix(1:4, 2)
stopifnot(identical(get_cells(m, j = 1), m[, 1, drop = FALSE]))
stopifnot(identical(stamp(3), 1:3))
stopifnot(identical(first_row(m), c(1L, 3L)))
stopifnot(identical(zero_at(1:3), c(0, 0, 0)))
message_of <- function(expr) tryCatch(expr, error = function(condition) conditionMessage(condition))
stopifnot(identical(message_of(get_elem(list(1))), "missing subscript"))
stopifnot(grepl("\"i\" is missing", message_of(drop_neg(1:3)), fixed = TRUE))
