# expect: RY080
# purrr typed-map with an incompatible callback return: map_dbl expects
# numeric returns but the callback produces character. R coerces
# silently at runtime; ry flags the likely-intended mismatch.
library(purrr)
xs <- map_dbl(1:3, function(x) paste("n", x))
