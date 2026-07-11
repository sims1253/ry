# oracle: must-pass
stopifnot(identical(list(1, 2) > 1, c(FALSE, TRUE)))
stopifnot(identical(list("NA") == "NA", TRUE))
