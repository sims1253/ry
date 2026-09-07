# oracle: must-warn RY091
# An omitted actual occupies f's position but cannot supply its required value.
result <- tryCatch(Filter(, x = 1L), error = identity)
stopifnot(inherits(result, "error"))
