foo <- function() "old"
foo <- base::identity(function() 1L)
box::export(foo)
