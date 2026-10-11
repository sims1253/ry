foo <- function() "old"
foo <- function() 1L
box::export(foo)
