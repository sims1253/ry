# expect: RY091, RY091, RY091, RY091
# With `robust` missing, an `||`/`|` condition with a `missing(robust)`
# disjunct is TRUE and an `&&`/`&` condition with a `!missing(robust)`
# conjunct is FALSE, so the guard's rebinding branch runs. `fit_real` has
# no missingness test, `fit_eager` and `fit_vector` evaluate both sides of
# `|`, and `fit_late` evaluates `is.null(robust)` first: each forces the
# omitted argument.
fit <- function(x, robust) {
  if (missing(robust) || is.null(robust)) robust <- FALSE
  if (robust) x <- x * 2
  x
}
fit_conj <- function(x, robust) {
  if (!missing(robust) && !is.null(robust)) NULL else robust <- FALSE
  if (robust) x <- x * 2
  x
}
plot_lim <- function(x, xlim, ylim) {
  if (missing(xlim) & missing(ylim)) {
    xlim <- range(x)
  } else if (missing(xlim) | missing(ylim)) {
    if (missing(xlim)) xlim <- ylim else ylim <- xlim
  }
  xlim[1]
}
fit_bound <- function(x, k) {
  if (missing(k) || k > length(x)) k <- length(x)
  x[1:k]
}
fit_plain <- function(x, robust) {
  if (missing(robust)) robust <- FALSE
  if (robust) x <- x * 2
  x
}
fit_real <- function(x, robust) {
  if (is.null(robust)) robust <- FALSE
  if (robust) x <- x * 2
  x
}
fit_eager <- function(x, robust) {
  if (missing(robust) | robust) robust <- FALSE
  if (robust) x <- x * 2
  x
}
fit_vector <- function(x, robust) {
  if (missing(robust) | is.null(robust)) robust <- FALSE
  if (robust) x <- x * 2
  x
}
fit_late <- function(x, robust) {
  if (is.null(robust) || missing(robust)) robust <- FALSE
  if (robust) x <- x * 2
  x
}
use <- function() {
  list(fit(1), fit_conj(1), plot_lim(1:3), fit_bound(1:3), fit_plain(1), fit_real(1), fit_eager(1))
}
use_order <- function() list(fit_vector(1), fit_late(1))
