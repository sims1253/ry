# no-diag
# `append()` grows the initially empty vector.
parts <- NULL
parts <- append(parts, "-")
if (parts[1] %in% c("-", "=")) print("part")

# A true conjunction proves the optional parameter is non-NULL.
f <- function(ref.group = NULL) {
  if (TRUE & !is.null(ref.group)) {
    if (ref.group %in% c("all", ".all.")) print("group")
  }
}

