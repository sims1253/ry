# oracle: must-pass
library(foreach)
registerDoSEQ()
options(foreachDoparLocal = TRUE)
x <- function() 1L
result <- foreach(i = 1:3) %dopar% { x <- 1L; x() }
stopifnot(identical(result, rep(list(1L), 3L)))
