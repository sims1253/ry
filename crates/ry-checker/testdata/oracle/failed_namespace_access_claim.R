# oracle: must-warn RY122
# oracle-claim: RY122
# Hide every non-base library so dplyr's namespace cannot load in this
# process: `requireNamespace(..., quietly = TRUE)` then returns FALSE,
# and a `dplyr::` access on that branch errors instead of falling back.
empty_lib <- tempfile("ry122lib")
dir.create(empty_lib)
.libPaths(empty_lib, include.site = FALSE)
stopifnot(identical(requireNamespace("dplyr", quietly = TRUE), FALSE))
pick <- function() if (!requireNamespace("dplyr", quietly = TRUE)) dplyr::mutate else identity
err <- tryCatch({
  pick()
  "no error"
}, error = function(e) conditionMessage(e))
stopifnot(grepl("there is no package called", err, fixed = TRUE))
