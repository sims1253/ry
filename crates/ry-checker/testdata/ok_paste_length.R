# no-diag
# paste/paste0 with longest_arg length inference: the result length
# matches the longest argument. Using the result with a string
# operation is well-typed.
x <- c("a", "b", "c")
y <- paste0(x, "_suffix")
z <- paste(x, collapse = ",")
