# no-diag
# Adjacent idioms around recognizing `return(...)` in the shared
# block-divergence view: only a direct `return` CALL exits, and only in
# the analyzed function's own body.

# A `return` inside a nested closure does not diverge the outer
# rejection block: it exits the closure (never called here), and the
# enclosing function continues on both paths, so the continuation
# after the `if` is not proven the guard-true path and RY110 stays
# silent at the demand.
closure_exit_only <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) {
    callback <- function(y) {
      return(y)
    }
  }
  sqrt(x)
}

# `return` as a passed value is an identifier, not a call: the
# rejection block below only binds a name, so nothing diverges.
return_as_value <- function(x) {
  if (!(is.numeric(x) || all(is.na(x)))) {
    holder <- return
  }
  sqrt(x)
}
