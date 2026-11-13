# oracle: must-flag
# Four nested sapply() callbacks return "x", so `v` is character and
# `v + 1L` errors in R (non-numeric argument to binary operator) (#896).
v <- sapply(1L, function(a) sapply(1L, function(b) sapply(1L, function(c) sapply(1L, function(d) "x"))))
v + 1L
