# oracle: must-pass
# A `$`/`[[` name missing from a list whose names ry only partly knows is
# not NULL in R: `list(...)` takes its names from the call, `lapply()` keeps
# the names of its input, and `$<-` on an argument adds to fields already
# there (#689).
f_dots <- function(...) {
  ddd <- list(...)
  if (is.null(ddd$legend)) TRUE else if (is.na(ddd$legend)) FALSE else TRUE
}
stopifnot(identical(f_dots(legend = NA), FALSE))

f_lapply <- function(x) {
  v <- lapply(x, as.numeric)
  if (v$major >= 3) TRUE else FALSE
}
stopifnot(isTRUE(f_lapply(list(major = "4", minor = "1"))))

f_lapply_known <- function() {
  v <- lapply(list(major = "4"), as.numeric)
  if (v$major >= 3) TRUE else FALSE
}
stopifnot(isTRUE(f_lapply_known()))

f_nested <- function(x) {
  x$leg$size <- c(1, 1)
  if (x$leg$horiz) TRUE else FALSE
}
stopifnot(isTRUE(f_nested(list(leg = list(horiz = TRUE)))))

f_removed <- function(x) {
  x$cex <- NULL
  if (x[[8]]) TRUE else FALSE
}
stopifnot(isTRUE(f_removed(list(cex = 1, 2, 3, 4, 5, 6, 7, 8, TRUE))))

registry <- new.env(parent = emptyenv())
registry$. <- FALSE
register <- function(key, fn) registry[[key]] <- list(fn = fn)
convert <- function(key, obj) {
  cv <- registry[[key]]
  if (is.null(cv)) obj else cv$fn(obj)
}
register("k", function(o) o * 10)
stopifnot(identical(convert("k", 2), 20))

halve <- function(o, p) {
  ee <- o$env
  ee$parameters <- list(b = 1)
  ee[[p]] / 2
}
stopifnot(identical(halve(list(env = list(last.par = c(a = 4))), "last.par"), c(a = 2)))
