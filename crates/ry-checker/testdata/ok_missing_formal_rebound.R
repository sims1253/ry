# no-diag
hypothesize <- function(x, null) {
  if (missing(null)) {
    null <- NA
  }
  null <- toupper(null)
  list(x, null)
}
hypothesize(1)
