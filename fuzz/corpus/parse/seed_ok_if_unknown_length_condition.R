# no-diag
# RY002 must not fire when the condition length is Unknown (only when
# Known(n > 1)). These patterns come from the glue vendor snapshot.
f <- function(x) { if (!inherits(x, "foo")) stop("nope"); x }
g <- function(flag) { if (flag) 1 else 2 }
h <- function(p) { if (isTRUE(p)) print("yes") }
k <- function(x) { if (isFALSE(x)) 1 else 2 }
