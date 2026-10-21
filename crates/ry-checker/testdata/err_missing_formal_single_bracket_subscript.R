# expect: RY091 RY091
# A missing formal passed bare to `[` is the empty subscript, not a read:
# `x[i, j]` with `i` omitted selects every row. `[[` has no empty
# subscript, and a compound subscript such as `-i` evaluates `i`, so only
# `get_elem` and `drop_neg` force their omitted formal.
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
use <- function(m) {
  list(get_cells(m, j = 1), stamp(3), first_row(m), zero_at(1:3), get_elem(list(1)), drop_neg(1:3))
}
