# oracle: must-warn RY091
# With `robust` omitted, `missing(robust) || ...` is TRUE and
# `!missing(robust) && ...` is FALSE, so each compound guard takes its
# rebinding branch and the function returns normally. `fit_real` has no
# missingness test, `fit_eager` and `fit_vector` evaluate both sides of
# `|`, and `fit_late` evaluates `is.null(robust)` before `missing()`: all
# error on the missing argument.
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
stopifnot(identical(fit(1), 1))
stopifnot(identical(fit_conj(1), 1))
stopifnot(identical(plot_lim(1:3), 1L))
stopifnot(identical(plot_lim(1:3, ylim = c(5, 6)), 5))
stopifnot(identical(fit_bound(1:3), 1:3))
missing_error <- function(call) {
  tryCatch(call, error = function(condition) conditionMessage(condition))
}
stopifnot(grepl("\"robust\" is missing", missing_error(fit_real(1)), fixed = TRUE))
stopifnot(grepl("\"robust\" is missing", missing_error(fit_eager(1)), fixed = TRUE))
stopifnot(grepl("\"robust\" is missing", missing_error(fit_vector(1)), fixed = TRUE))
stopifnot(grepl("\"robust\" is missing", missing_error(fit_late(1)), fixed = TRUE))
