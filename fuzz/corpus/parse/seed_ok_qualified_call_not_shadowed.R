# no-diag
# A namespace-qualified call must not be shadowed by a same-named local
# non-function binding.
render <- function(autofit = TRUE) {
  if (autofit) {
    flextable::autofit()
  }
}
