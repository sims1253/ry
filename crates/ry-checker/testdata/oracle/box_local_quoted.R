# oracle: must-pass
# A module's quoting formal takes precedence over an identically named
# installed package function and does not evaluate this missing symbol.
box::use(./box_module/local_filter[filter])
d <- data.frame(x = 1L)
result <- filter(d, missing_column)
stopifnot(identical(result, d))
