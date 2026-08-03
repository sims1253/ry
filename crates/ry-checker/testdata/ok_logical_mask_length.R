# no-diag
a <- function(units) {
  day.units <- c("day", "wday", "mday", "yday")
  wunit <- day.units %in% names(units)
  n <- sum(wunit)
  if (n > 0) {
    if (n > 1) stop("conflicting days input")
    uname <- day.units[wunit]
    if (uname != "mday") 1 else 2
  }
}
