# no-diag
# A same-named wrapper around another package's function is not recursion:
# the qualified call keeps its unknown result instead of being dropped.
normalize_path <- function(path) {
  if (!is.null(path)) xfun::normalize_path(path)
}
if (normalize_path(".") == normalize_path("./")) message("same")
