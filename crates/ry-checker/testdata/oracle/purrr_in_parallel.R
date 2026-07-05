# oracle: must-pass
# purrr::map + in_parallel (>= 1.1.0): R succeeds and ry must check the
# inner callback identically to the sequential form.
library(purrr)
sims <- list(1, 2, 3)
out <- map(sims, in_parallel(function(s) s[[1]] * 2))
print(out)
