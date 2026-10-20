# oracle: must-pass
# Forcing `y` binds a local `f`, so the call reaches `paste` (S6).
f <- function(x) x + 1L
h <- function(y = { f <- paste; NULL }) { y; f("a") }
stopifnot(identical(h(), "a"))
