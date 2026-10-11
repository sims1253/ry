# no-diag
# An alias of a box-imported package function keeps that function's
# data-mask contract.
box::use(dplyr[filter])
d <- data.frame(mpg = c(21, 22.8))
keep <- filter
keep(d, mpg > 21)
