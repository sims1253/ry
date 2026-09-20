# oracle: must-pass
# oracle-claim: RY106
# The recommendation RY106 makes must name a real, exported, typed
# function. `vctrs::if_else()` does not exist (vctrs 0.7.3 has
# `vec_if_else()`, added in 0.7.0); the claim fixture pins the
# alternative the diagnostic now suggests, `dplyr::if_else()`, by
# executing it on the exact shapes where `ifelse()` collapses: the
# result keeps the branch mode for an empty condition and for an
# all-NA condition, an NA condition entry yields NA (or the explicit
# `missing` value), and a nonempty mixed condition selects per entry.
# The `ifelse()` collapse examples are restated first so the contrast
# is demonstrated by one executed file.
stopifnot(identical(typeof(ifelse(logical(0), NA_character_, "a")), "logical"))
stopifnot(identical(ifelse(logical(0), NA_character_, "a"), logical(0)))
stopifnot(identical(typeof(ifelse(rep(NA, 2), "a", "b")), "logical"))
stopifnot(exists("if_else", where = asNamespace("dplyr"), inherits = FALSE))
stopifnot("if_else" %in% getNamespaceExports("dplyr"))
stopifnot(identical(dplyr::if_else(logical(), "a", "b"), character(0)))
stopifnot(identical(
  dplyr::if_else(rep(NA, 2), "a", "b"),
  c(NA_character_, NA_character_)
))
stopifnot(identical(
  dplyr::if_else(c(TRUE, FALSE, NA), "a", "b"),
  c("a", "b", NA_character_)
))
stopifnot(identical(
  dplyr::if_else(c(TRUE, FALSE, NA), "a", "b", missing = "m"),
  c("a", "b", "m")
))
stopifnot(identical(dplyr::if_else(c(TRUE, FALSE), "a", "b"), c("a", "b")))
