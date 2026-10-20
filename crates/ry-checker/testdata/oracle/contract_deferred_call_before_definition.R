# oracle: must-flag-only RY092
# A deferred call reaches the later global definition when it runs.
g <- function() f("a")
f <- function(x) x + 1L
g()
