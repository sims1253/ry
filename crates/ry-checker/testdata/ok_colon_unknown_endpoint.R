# no-diag
# Idioms built on `a:b` with unknown endpoints stay quiet once `1:n` is
# typed as a nonempty integer-or-double vector (#648). `a:b` with two
# endpoints of unknown class may be a factor interaction, so it stays
# unknown.
use_ranges <- function(n, x) {
  out <- numeric(n)
  for (i in 1:n) {
    out[i] <- x[i] * 2
  }
  y <- x[1:n]
  z <- 1:n * 2
  w <- paste0("a", 1:n)
  k <- (1:n) / 2
  d <- n:1 - 1L
  s <- seq_along(x)
  list(out, y, z, w, k, d, s, sum(1:n), rev(1:n), letters[1:n])
}

interact <- function(a, b) {
  ab <- a:b
  levels(ab)
}

from_text <- function(to) {
  "1":to
}
