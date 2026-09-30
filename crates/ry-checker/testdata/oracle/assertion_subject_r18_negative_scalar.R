# oracle: must-pass
f <- function() { x <- 1L; for(i in integer()) x <- 1L; x < -1L && TRUE };
stopifnot(!f())
