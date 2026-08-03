# no-diag
# A self-referential default is legal when the promise is captured without
# forcing it. rlang uses this shape to test enexpr()/enquo() defusing.
expr_default <- function(x = x) rlang::enexpr(x)
quo_default <- function(x = x) list(rlang::enquo(x), rlang::quo(x))
expr_default()
quo_default()
