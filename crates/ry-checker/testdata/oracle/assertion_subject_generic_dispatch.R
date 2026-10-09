# oracle: must-warn RY032
# A generic dispatches to a project method that rebinds the caller's subject.
# Rebind `x` in the nearest calling frame that defines `target_frame`.
replace_x <- function() {
  for (frame in rev(sys.frames())) {
    if (exists("target_frame", envir = frame, inherits = FALSE)) {
      return(assign("x", c(1L, 2L), envir = frame))
    }
  }
}
print.foo <- function(x, ...) {
  replace_x()
  invisible(x)
}
as.character.foo <- function(x, ...) {
  replace_x()
  "foo"
}
setClass("bar", representation(v = "numeric"))
setMethod("show", "bar", function(object) {
  replace_x()
})
printed <- function(z, x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  print(z)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
converted <- function(z, x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  as.character(z)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
shown <- function(z, x = 1L) {
  target_frame <- TRUE
  stopifnot(x > 0 && TRUE)
  show(z)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
foo <- structure(1, class = "foo")
for (run in list(
  function() printed(foo),
  function() converted(foo),
  function() shown(new("bar", v = 1))
)) {
  vector_error <- tryCatch(run(), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
