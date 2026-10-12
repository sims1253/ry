#' @export
foo <- 1L
bar <- 2L
do.call(box::export, list(quote(bar)))
