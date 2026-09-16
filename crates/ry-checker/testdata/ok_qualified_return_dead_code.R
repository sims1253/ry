# no-diag
# Code after base::return(...) is dead exactly as after bare return(...):
# the walker's Stmt::Expr return arm recognizes the `::`-qualified callee
# through bare_name (the same view expr_diverges takes), so the condition
# and the unbound read below are never walked and stay quiet. The bare
# twin has always silenced them; the qualified twin must not start firing.
bare <- function(x) {
  return(x)
  if ("a") NULL
  undefined_name
}
qualified <- function(x) {
  base::return(x)
  if ("a") NULL
  undefined_name
}
bare(1L)
qualified(1L)
