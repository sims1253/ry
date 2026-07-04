# no-diag
# purrr typed-map: map_dbl returns a double vector of .x's length.
# Compatible callback returns (numeric coercions) are accepted.
library(purrr)
dbls <- map_dbl(1:3, function(x) x + 0.5)
ints <- map_int(1:3, function(x) as.integer(x))
lgls <- map_lgl(1:3, function(x) x > 1)
