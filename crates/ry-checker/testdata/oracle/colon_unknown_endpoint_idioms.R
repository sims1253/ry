# oracle: must-pass
# Common uses of `a:b` with unknown endpoints run in R. Typing `1:n` as a
# nonempty integer-or-double vector must keep them quiet, and two
# endpoints of unknown class may be factors whose `:` is their
# interaction, a factor with levels (#648).
use_ranges <- function(n, x) {
  out <- numeric(n)
  for (i in 1:n) out[i] <- x[i] * 2
  y <- x[1:n]
  z <- 1:n * 2
  w <- paste0("a", 1:n)
  k <- (1:n) / 2
  d <- n:1 - 1L
  stopifnot(length(1:n) >= 1L, identical(sum(1:n), sum(seq_len(n))))
  list(out, y, z, w, k, d, rev(1:n), letters[1:n])
}
r <- use_ranges(3, c(1, 2, 3))
stopifnot(identical(r[[4]], c("a1", "a2", "a3")))

interact <- function(a, b) {
  ab <- a:b
  stopifnot(is.factor(ab))
  levels(ab)
}
lv <- interact(factor(c("a", "b")), factor(c("x", "y")))
stopifnot(identical(lv, c("a:x", "a:y", "b:x", "b:y")))

# Character endpoints that parse as numbers are coerced.
from_text <- function(to) "1":to
stopifnot(identical(from_text("3"), 1:3))
# `1:0` has two elements; `:` never returns an empty vector.
stopifnot(identical(length(1:0), 2L))
# R: typeof(1.5:n) is "double" and typeof(1:3e9) is "double".
stopifnot(identical(typeof(1.5:3), "double"), identical(typeof(1:3e9), "double"))
