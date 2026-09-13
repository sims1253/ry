# oracle: must-flag
lapply(1:3, function(i) { x <- 1L; x() })
x <- function() 2L
