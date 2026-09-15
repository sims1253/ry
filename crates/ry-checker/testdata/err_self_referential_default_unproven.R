# expect: RY109, RY109, RY109, RY109
# dtplyr's pre-fix shapes (parent commit bffe46e, fixed upstream in dbe32a6
# "Fix recursive helper defaults"): conditional or forwarded forcing is not a
# forcing proof, yet the default recurses the moment it is triggered, so
# RY109 warns.
is_step <- function(x) FALSE
lazy_dt <- function(x) x
across_setup <- function(data, call, env, ...) list()
dtplyr_auto_copy <- function(x, y, copy = copy) {
  if (is_step(y)) {
    y
  } else if (is.data.frame(y)) {
    lazy_dt(y)
  } else {
    dplyr::auto_copy(x, y, copy = copy)
  }
}
dt_squash_across <- function(call, env, data, j = j, is_top = TRUE) {
  across_setup(data, call, env, allow_rename = TRUE, j = j, fn = "across()")
}
compound <- function(n = n + 1) list(n)
never_used <- function(copy = copy) 1L
