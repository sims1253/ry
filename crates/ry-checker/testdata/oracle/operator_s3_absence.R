# oracle: must-pass
# Arithmetic on a classed numeric with no `Ops` method anywhere uses R's
# internal primitive rules: R computes it fine, and ry must neither flag
# an error nor report a missing S3 method (RY050) for it.
x <- structure(c(1, 2), class = "local_record")
total <- x + x
stopifnot(is.numeric(total), length(total) == 2)
