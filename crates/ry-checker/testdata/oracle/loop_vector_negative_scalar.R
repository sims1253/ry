# oracle: must-pass
# A scalar entry value carries no vector path through the empty loop.
f <- function() { x <- 1L; for(i in integer()) x <- 1L; x < -1L && TRUE };
stopifnot(!f())
