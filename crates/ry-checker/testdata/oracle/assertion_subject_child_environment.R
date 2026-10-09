# oracle: must-warn RY032
# Code evaluated in a child environment can rebind the caller's subject,
# before or after the assertion.
local_assign <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  local(assign("x", c(1L, 2L), envir = parent.env(environment())))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
with_superassign <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  with(list(), x <<- c(1L, 2L))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
within_superassign <- function(x = 1L) {
  stopifnot(x > 0 && TRUE)
  within(list(), x <<- c(1L, 2L))
  if (is.null(x) || x == 1L) TRUE else FALSE
}
local_active <- function(x = NULL) {
  local({
    frame <- parent.env(environment())
    rm("x", envir = frame)
    reads <- 0L
    makeActiveBinding("x", function() {
      reads <<- reads + 1L
      if (reads == 1L) 1L else c(1L, 2L)
    }, frame)
  })
  stopifnot(x > 0 && TRUE)
  if (is.null(x) || x == 1L) TRUE else FALSE
}
for (f in list(local_assign, with_superassign, within_superassign, local_active)) {
  vector_error <- tryCatch(f(), error = function(e) conditionMessage(e))
  stopifnot(grepl("length = 2", vector_error, fixed = TRUE))
}
