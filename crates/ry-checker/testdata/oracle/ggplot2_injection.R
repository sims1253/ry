# oracle: must-pass
if (requireNamespace("ggplot2", quietly = TRUE)) {
  mapping <- ggplot2::aes(!!!stats::setNames(lapply(c("mpg", "wt"), as.name), c("x", "y")))
  stopifnot(identical(rlang::quo_get_expr(mapping$x), quote(mpg)))
  stopifnot(identical(rlang::quo_get_expr(mapping$y), quote(wt)))
  mapping <- ggplot2::aes(x = !!quote(mpg), y = !!quote(wt))
  stopifnot(identical(rlang::quo_get_expr(mapping$x), quote(mpg)))
  facets <- ggplot2::vars(!!!list(quote(mpg), quote(wt)))
  stopifnot(identical(rlang::quo_get_expr(facets[[1L]]), quote(mpg)))
  stopifnot(identical(rlang::quo_get_expr(facets[[2L]]), quote(wt)))
  facet <- ggplot2::vars(!!quote(mpg))
  stopifnot(identical(rlang::quo_get_expr(facet[[1L]]), quote(mpg)))
}
