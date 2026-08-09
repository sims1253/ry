# oracle: must-pass
# Out-of-bounds single-bracket indexing returns NA, not an error, in R.
x <- c(1, 2)[5]
