# oracle: must-warn RY032
f <- function(xs) { x <- c(1L, 2L); for (i in xs) x <- 1L; y <- x; if (1L == y && TRUE) y }; vector_error <- tryCatch(do.call(f, list(integer())), error = function(e) conditionMessage(e)); stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
