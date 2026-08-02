# no-diag
# R truncates double subscripts before indexing, so c(0.5, 1.5) selects one
# element rather than proving a result of the index vector's length.
x <- c(TRUE, FALSE)
y <- x[c(0.5, 1.5)]
if (y) 1L else 0L
