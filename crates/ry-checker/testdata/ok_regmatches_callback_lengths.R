# no-diag
x <- c("ab", "z")
matches <- regmatches(x, regexec("(a)(b)", x))
Filter(function(z) length(z) > 0L, matches)
