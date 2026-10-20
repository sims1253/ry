# expect: RY091
# A `$`/`@` member name is not a read of the formal with that name, and the
# inner targets of a chained assignment are frame rebindings, not reads.
# Only `fit_real` forces its omitted `tt`.
fit <- function(formula, tt) {
  timetrans <- attr(formula, "specials")$tt
  if (missing(tt)) tt <- NULL
  list(timetrans, tt)
}
fit_slot <- function(obj, tt) {
  timetrans <- obj@tt
  if (missing(tt)) tt <- NULL
  list(timetrans, tt)
}
fit_bracket <- function(formula, tt) {
  timetrans <- attr(formula, "specials")[["tt"]]
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
init_split <- function(ai, bi, ci) {
  ai <- NA_real_; bi <- NA_real_; ci <- NA_real_
  c(ai, bi, ci)
}
fit_real <- function(formula, tt) formula$tt + tt
use <- function(f, o) {
  list(fit(f), fit_slot(o), fit_bracket(f), init(), init_equals(), init_split(), fit_real(f))
}
