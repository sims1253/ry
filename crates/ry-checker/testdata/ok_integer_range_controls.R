# no-diag
as.integer(2147483647.9)
as.integer(-2147483647.9)
as.integer(NaN)
as.integer(NA_real_)
x <- as.integer(1e10)
x[is.na(x)] <- 0L
local_cast <- function(x) 1L
as.integer <- local_cast
as.integer(1e10)
