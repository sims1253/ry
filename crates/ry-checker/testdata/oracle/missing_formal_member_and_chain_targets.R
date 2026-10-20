# oracle: must-warn RY091
# A `$` or `@` member name does not force the formal of the same name, and
# the inner targets of `a <- b <- v` rebind the frame without reading the
# argument: each guarded function returns normally with `tt` or `bi`/`ci`
# omitted. Only `fit_real`, which reads `tt` itself, errors.
fit <- function(formula, tt) {
  timetrans <- attr(formula, "specials")$tt
  if (missing(tt)) tt <- NULL
  list(timetrans, tt)
}
setClass("Ry685Terms", representation(tt = "numeric"))
fit_slot <- function(obj, tt) {
  timetrans <- obj@tt
  if (missing(tt)) tt <- NULL
  list(timetrans, tt)
}
init <- function(ai, bi, ci) {
  ai <- bi <- ci <- NA_real_
  c(ai, bi, ci)
}
init_equals <- function(ai, bi) {
  ai = bi = NA_real_
  c(ai, bi)
}
fit_real <- function(formula, tt) formula$tt + tt
stopifnot(identical(fit(y ~ x), list(NULL, NULL)))
stopifnot(identical(fit_slot(new("Ry685Terms", tt = 1)), list(1, NULL)))
stopifnot(identical(init(), rep(NA_real_, 3L)))
stopifnot(identical(init_equals(), rep(NA_real_, 2L)))
error <- tryCatch(fit_real(list(tt = 1)), error = function(condition) conditionMessage(condition))
stopifnot(grepl("\"tt\" is missing", error, fixed = TRUE))
