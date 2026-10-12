# no-diag
# A recursive return whose base case is unknown stays unknown rather than
# taking its type from another branch.
walk <- function(n, x) if (n == 0L) x else walk(n - 1L, x)
walk(2L, 1) + 1L
walk(2L, "a") == "a"
