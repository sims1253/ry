# oracle: must-warn RY032
# An empty loop keeps the vector; a negative literal comparison keeps its length.
f <- function() { x <- c(1L, 2L); for(i in integer()) x <- 1L; x < -1L && TRUE };
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
