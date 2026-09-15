# no-diag
# A self-referential default is deliberate when the body defuses the promise
# with a reviewed capture helper or tidy injection: the defuser receives the
# unevaluated default, so these run. Typeshed provenance proves the defusing,
# so RY109 stays quiet alongside RY098.
expr_default <- function(x = x) rlang::enexpr(x)
quo_default <- function(x = x) list(rlang::enquo(x), rlang::quo(x))
base_default <- function(x = x) substitute(x)
tidy_default <- function(data, x = x) dplyr::mutate(data, col = f({{ x }}))
expr_default()
quo_default()
base_default()
