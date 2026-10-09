foo <- function() "old"
dummy <- (foo <- function() 1L)
box::export(foo)
