# no-diag
# RY111 suppression gates: every shape that must stay silent.
# Gate 1 -- identical name. A differently-named enclosing formal is the
# deliberate renaming-and-defaulting idiom, and a partial tag (`na.r`)
# is not the exact spelling.
f_rename <- function(x, remove_na = FALSE) {
  median(x, na.rm = TRUE)
}
f_partial_tag <- function(x, na.rm = FALSE) {
  median(x, na.r = TRUE)
}
# Gate 2 -- the enclosing scope must expose the formal at all. Top-level
# code and formal-less wrappers hardcode legitimately.
median(1:3, na.rm = TRUE)
f_no_formal <- function(x) {
  median(x, na.rm = TRUE)
}
# Gate 3 -- the value must be the reserved-word literal. Forwarding the
# formal, computed expressions, and the rebindable `T`/`F` spellings stay
# quiet, as does `NA` (a typed hole, not a hardcoded policy).
f_forward <- function(x, na.rm = FALSE) {
  median(x, na.rm = na.rm)
}
f_expr <- function(x, na.rm = FALSE) {
  median(x, na.rm = as.logical(na.rm))
}
f_T <- function(x, na.rm = FALSE) {
  median(x, na.rm = T)
}
f_NA <- function(x, na.rm = FALSE) {
  median(x, na.rm = NA)
}
# Gate 4 -- the callee must resolve to a signature that has the formal.
# A `...`-only callee forwards the literal into dots (destination unknown),
# and an unknown callee resolves to no formals at all.
variadic <- function(x, ...) list(x, ...)
f_dots_callee <- function(x, na.rm = FALSE) {
  variadic(x, na.rm = TRUE)
}
f_unknown_callee <- function(x, na.rm = FALSE) {
  not_collected_anywhere(x, na.rm = TRUE)
}
# Gate 5 -- logical literals only. Numeric and string constants carry a
# deliberate divergent-default idiom (`sep = ","` reformatted to `"\t"`)
# that the name match cannot distinguish from the mistake.
rep_times <- function(x, times = 3) {
  rep(x, times = 3)
}
paste_sep <- function(x, sep = ",") {
  paste(x, collapse = "+")
}
paste_sep_inner <- function(x, sep = ",") {
  paste(x, collapse = "-")
}
paste0_sep <- function(x, sep = ",") {
  paste0(x, sep = "")
}
# Gate 6 -- the callee formal must match exactly, not absorb through
# partial matching at the signature level: `na.r`-style tags never bind
# the identical name.
f_partial_signature <- function(x, na.rm = FALSE) {
  sum(x, na.r = TRUE)
}
# Gate 7 -- the dead-formal gate: a body that reads the formal anywhere
# (guard, validation, by-name forward at another site, missing() test)
# handles the caller's value, so the per-site constant is deliberate
# child semantics. These are the corpus idioms: stringr's guarded
# ignore_case, dbplyr's forwarded subquery, tibble's forwarded quiet,
# dplyr's guarded recursive, rvest's env_has inherit.
f_guarded <- function(x, ignore_case = FALSE) {
  if (ignore_case) {
    return(x)
  }
  median(x, ignore_case = FALSE)
}
final <- function(x, quiet = FALSE) x
f_forwarded_elsewhere <- function(x, quiet = FALSE) {
  a <- median(x, quiet = TRUE)
  final(a, quiet = quiet)
}
f_validated <- function(x, na.rm = FALSE) {
  check_bool <- function(flag) NULL
  check_bool(na.rm)
  median(x, na.rm = TRUE)
}
f_missing_test <- function(x, keep = FALSE) {
  if (!missing(keep)) {
    warning("keep is ignored")
  }
  median(x, keep = TRUE)
}
f_captured_read <- function(x, na.rm = FALSE) {
  g <- function(y) {
    if (na.rm) y else y
  }
  g(x)
  median(x, na.rm = TRUE)
}
# A read inside a return value is still a read, and a formal default
# naming a sibling formal consumes the caller's value.
f_return_read <- function(x, na.rm = FALSE) {
  return(if (na.rm) x else median(x, na.rm = FALSE))
}
f_default_reads_sibling <- function(x, na.rm = FALSE, force = na.rm) {
  median(x, na.rm = TRUE)
}
