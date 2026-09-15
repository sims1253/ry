# expect: RY107
# glue R/utils.R:32 (da9c73f). `any()` returns a length-1 logical, so
# `any(lengths) == 0` computes `!any(lengths)` -- FALSE == 0 is TRUE -- and
# the emptiness guard runs on the wrong condition. The same commit writes
# the intended form twice in R/glue.R:139,191. The remaining lines are the
# adjacent idioms that must stay quiet: the element-level spelling, the
# value-preserving scalar comparisons diffobj writes on purpose
# (`!all(diff(x)) == 1L`, pinned in ry095_ry096_real_shapes.R), and a
# multi-argument any() whose na.rm control the suggested rewrite cannot
# carry.
recycle_columns <- function(df) {
  lengths <- vapply(df, NROW, integer(1))
  if (any(lengths) == 0) {
    return(character())
  }
  df
}
recycle_columns_fixed <- function(df) {
  lengths <- vapply(df, NROW, integer(1))
  if (any(lengths == 0)) {
    return(character())
  }
  df
}
diffobj_shape <- function(x) {
  !all(diff(x)) == 1L
}
ok_scalar <- function(x) {
  any(x) == 1
}
ok_counting <- function(x) {
  sum(x > 0)
}
ok_na_rm <- function(x) {
  any(x, na.rm = TRUE) == 0
}
