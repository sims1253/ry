foo <- function() "old"
replacement <- function() 1L
foo <- replacement
box::export(foo)
