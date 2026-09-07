# oracle: must-pass
# A visible data.frame method does not replace hist.default for numeric input.
hist.data.frame <- function(x, ...) 1
plain <- hist(1:10, plot = FALSE)
other <- hist(structure(1:10, class = "ry_hist_other"), plot = FALSE)
stopifnot(is.numeric(plain$breaks), identical(plain$breaks, other$breaks))
