# no-diag
# purrr in_parallel (>= 1.1.0) is a type-transparent wrapper:
# map(sims, in_parallel(f)) checks identically to map(sims, f).
library(purrr)
sims <- list(1, 2, 3)
results <- map(sims, in_parallel(function(s) s[[1]] * 2))
dbls <- map_dbl(sims, in_parallel(function(s) s[[1]] + 0.5))
