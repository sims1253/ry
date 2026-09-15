# no-diag
# Adjacent vacuous-all idioms that must stay quiet.
#
# The fixed hms form (046414d): the emptiness is guarded before all().
fixed_hms <- function(x) {
  if (is.numeric(x) || (length(x) > 0 && all(is.na(x)))) {
    sqrt(x)
  }
}
# No downstream mode demand: a permissive guard with only
# type-agnostic consumers is intentionally fine.
no_demand <- function(x) {
  if (is.numeric(x) || all(is.na(x))) {
    paste(x, collapse = ",")
  }
}
# A demand that accepts every vacuous mode is no failure.
accepting_demand <- function(x) {
  if (is.numeric(x) || all(is.na(x))) {
    print(x)
  }
}
# `&&` guards do not accept vacuously: a FALSE predicate already
# rejects the value.
and_guard <- function(x) {
  if (is.numeric(x) && all(is.na(x))) {
    sqrt(x)
  }
}
# A bare `all(is.na(x))` guard is the skip-logic idiom, not a
# validation alternatives guard.
skip_logic <- function(x) {
  if (all(is.na(x))) {
    return(NA_real_)
  }
  sqrt(x)
}
# Proven-nonempty values cannot hit the vacuous path.
v <- c(1, 2)
nonempty_local <- if (is.numeric(v) || all(is.na(v))) sqrt(v)
# A binding whose recorded mode is covered by the predicate admits
# nothing vacuously: its empty value is covered too.
covered_mode <- function(x) {
  x <- as.numeric(x)
  if (is.numeric(x) || all(is.na(x))) {
    sqrt(x)
  }
}
# Rebinding between guard and demand replaces the guarded value.
rebound <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  x <- 1
  sqrt(x)
}
# A shadowed `all` is not the base aggregate.
all <- function(x) TRUE
own_all <- function(x) {
  if (is.numeric(x) || all(is.na(x))) {
    sqrt(x)
  }
}
# A non-diverging rejection block leaves the accepted path unproven.
soft_reject <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) {
    x <- NULL
  }
  sqrt(x)
}
# `any(is.na(x))` is FALSE over empty; it rejects rather than accepts.
any_guard <- function(x) {
  if (is.numeric(x) || any(is.na(x))) {
    sqrt(x)
  }
}
# A nested closure's identically named parameter is a different
# binding; its demand never sees the guarded value.
shadowed_closure <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) stop("bad")
  helper <- function(x) sqrt(x)
  helper(2)
}
# vapply over a vacuously accepted empty input returns numeric(0)
# without ever invoking the lambda; no failure path exists.
shadowed_vapply <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  out <- vapply(x, function(x) sqrt(x), numeric(1))
}
# A loop variable rebinds the name for the body and afterwards; the
# guarded input never reaches the demand (the loop's integer values do,
# which the demand accepts).
loop_rebind <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  for (x in c(1, 2)) total <- x
  sqrt(x)
}
# Subassign and assign() rebind the value like a plain assignment does.
subassign_rebind <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  x[1] <- 2
  sqrt(x)
}
assign_rebind <- function(x) {
  stopifnot(is.numeric(x) || all(is.na(x)))
  assign("x", 1)
  sqrt(x)
}
# A predicate over another variable does not make this a guard over x;
# there is no predicate over the `all()` variable at all.
foreign_predicate <- function(x, y) {
  if (is.character(y) || all(is.na(x))) sqrt(x)
}
# `invisible()` returns a value and does not exit: the continuation is
# not provably the accepted path.
invisible_reject <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) invisible(NULL)
  sqrt(x)
}
