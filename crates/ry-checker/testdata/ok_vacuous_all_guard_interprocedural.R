# no-diag
# Interprocedural vacuous-all shapes that must stay quiet (issue #479).
# The demand gate is exactly as strict as the inline rule's: no demand,
# an accepting demand, or a fixed helper means no diagnostic.
is_numeric_or_na <- function(x) is.numeric(x) || all(is.na(x))
# A discarded helper result validates nothing.
discarded <- function(v) {
  is_numeric_or_na(v)
  sqrt(v)
}
# No downstream mode demand: a permissive guard with only
# type-agnostic consumers is intentionally fine.
no_demand <- function(v) {
  if (!is_numeric_or_na(v)) stop("bad")
  paste(v, collapse = ",")
}
# A demand that accepts every vacuous mode is no failure.
accepting_demand <- function(v) {
  if (!is_numeric_or_na(v)) stop("bad")
  print(v)
}
# The fixed helper form (hms 046414d) registers nothing: its `all()`
# is guarded by nonemptiness.
fixed_helper <- function(x) {
  is.numeric(x) || (length(x) > 0 && all(is.na(x)))
}
fixed_caller <- function(v) {
  if (!fixed_helper(v)) stop("bad")
  sqrt(v)
}
# A multi-formal function is not a guard-helper, even when its body is
# the chain over the first formal.
multi_formal <- function(x, strict) is.numeric(x) || all(is.na(x))
multi_caller <- function(v) {
  if (!multi_formal(v)) stop("bad")
  sqrt(v)
}
# A shadowed helper name at the call site never reaches the registered
# definition.
shadowed_helper <- function(v) {
  is_numeric_or_na <- function(x) TRUE
  if (!is_numeric_or_na(v)) stop("bad")
  sqrt(v)
}
# Rebinding the actual between guard and demand replaces the guarded
# value.
rebound_actual <- function(v) {
  if (!is_numeric_or_na(v)) stop("bad")
  v <- 1
  sqrt(v)
}
# Rebinding the mapped collection voids the `all(valid)` verdicts.
rebound_mapped <- function(args) {
  valid <- map_lgl(args, is_numeric_or_na)
  if (!all(valid)) stop("bad")
  args <- 1
  sqrt(args)
}
# Extra call arguments do not bind the helper's single formal.
extra_arg <- function(v) {
  if (!is_numeric_or_na(v, TRUE)) stop("bad")
  sqrt(v)
}
# A non-diverging rejection block leaves the accepted path unproven,
# exactly like the inline rule.
soft_reject <- function(v) {
  if (!is_numeric_or_na(v)) v <- NULL
  sqrt(v)
}
