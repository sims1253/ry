# oracle: must-pass
# purrr::map applies a function over a list; R succeeds (purrr attached).
library(purrr)
out <- map(list(1, 2, 3), function(x) x * 2)
print(out)
