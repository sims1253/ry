# oracle: must-warn RY034
result <- 1 == NA
stopifnot(length(result) == 1, is.na(result))
