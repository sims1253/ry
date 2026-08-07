# expect: RY102, RY103, RY105
# Plan 31 W18 recall rules, reproduced from `docs/plans/repro/31/fn.R`.
# Each line is the minimal form of a real defect the 62-package Posit corpus
# audit found and 0.8.0 missed entirely (the repro file checked clean).
#
# A1 — pak R/pak-sitrep-data.R:41. `names()` of this list is c("ref", "", "sha").
l <- list(ref = "a", "github-ref" <- "b", sha = "c")
# A3 — sparklyr R/worker_apply.R:522. `class()` is a vector, so `&&` errors
# with 'length = 2' in coercion to logical(1) for any multi-class object.
k <- function(df, t) {
  if (!is.null(t) && class(df[[1]]) != t) 1
}
# A7 — glue R/utils.R:32. A real bug: the author meant `any(lengths == 0)`,
# which the same package writes correctly at R/glue.R:139. Deliberately still
# MISSED. The plan justified a rule here with "is always FALSE", which is
# wrong -- `any()` returns a logical and `FALSE == 0` is TRUE -- and the shape
# cannot be told apart from diffobj's legitimate `!all(diff(x)) == 1L`, pinned
# as must-stay-silent in ry095_ry096_real_shapes.R. Left open rather than
# traded for a false-positive source.
u <- function(x) {
  lengths <- vapply(x, NROW, integer(1))
  if (any(lengths) == 0) {
    return(character())
  }
  1
}
# A7b — pak R/confirmation.R:42. `sum()` is length 1, so the guard is always
# TRUE and the "nothing unknown" path is dead.
p <- function(s) {
  u_dl <- sum(is.na(s))
  any_unk <- length(u_dl) > 0
  any_unk
}
