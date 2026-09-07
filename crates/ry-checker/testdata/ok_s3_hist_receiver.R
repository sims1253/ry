# no-diag
hist.data.frame <- function(x, ...) 1
result <- hist(1:10, plot = FALSE)
result$breaks
