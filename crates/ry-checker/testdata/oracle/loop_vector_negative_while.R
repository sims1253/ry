# oracle: must-warn RY032
# A loop that may not run keeps the entry vector for a negative comparison.
f <- function(flag=FALSE) { x <- c(1L, 2L); while(flag) { x <- 1L; flag <- FALSE }; x < -1L && TRUE };
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
