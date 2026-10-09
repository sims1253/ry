# oracle: must-warn RY032
# The vector path survives a mirrored comparison with a negative literal.
f <- function(flag=FALSE) { x <- c(1L, 2L); while(flag) { x <- 1L; flag <- FALSE }; -1L < x && TRUE };
vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
