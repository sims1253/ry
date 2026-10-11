foo <- function() "old"
rm(foo)
foo <- base::identity(function() 1L)
box::export(foo)
