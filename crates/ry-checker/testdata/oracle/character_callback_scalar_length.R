# oracle: must-warn RY105
x <- c("", "ab")
kept <- Filter(function(z) length(z) > 0L, x)
stopifnot(identical(kept, x))
