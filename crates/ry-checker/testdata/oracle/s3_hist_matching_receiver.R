# oracle: must-flag
# The same method applies when the receiver actually is a data.frame.
hist.data.frame <- function(x, ...) 1
result <- hist(data.frame(x = 1:10))
result$breaks
