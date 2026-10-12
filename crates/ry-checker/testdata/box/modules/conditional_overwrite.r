foo <- function() "old"
if (TRUE) foo <- function() 1L
box::export(foo)
