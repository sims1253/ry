# oracle: must-flag
# Sequential execution replaces the function in the caller's frame.
library(foreach)
x <- function() 1L
foreach(i = 1:3) %do% { x <- 1L; x() }
