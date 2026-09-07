# oracle: must-pass
x <- c("ab", "z")
matches <- regmatches(x, regexec("(a)(b)", x))
kept <- Filter(function(z) length(z) > 0L, matches)
stopifnot(identical(lengths(matches), c(3L, 0L)))
stopifnot(identical(kept, list(c("ab", "a", "b"))))
