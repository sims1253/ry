# no-diag
# A dynamic write makes a record schema incomplete.
x <- list(known = 1L)
key <- "runtime"
x[[key]] <- 2L
x$another_runtime_field
