# oracle: must-warn RY094
result <- tryCatch(sprintf('%d %d', , 1L), error=identity)
stopifnot(inherits(result, 'error'))
