# oracle: must-pass
# length evaluates its sole argument before primitive/S3 dispatch code.
# A forcing contract does not establish purity after that evaluation.
local({
  events <- new.env()
  events$values <- character()
  length.widget <- function(x) {
    events$values <- c(events$values, "method")
    stopifnot(inherits(x, "widget"))
    1L
  }
  eager <- function(p) { base::length(p) }
  result <- eager({
    events$values <- c(events$values, "argument")
    structure(1L, class = "widget")
  })
  stopifnot(identical(result, 1L))
  stopifnot(identical(events$values, c("argument", "method")))

  suffix <- function(p = { x <- "changed"; 1L }) {
    x <- 1L
    base::length(p)
    x
  }
  stopifnot(identical(suffix(), "changed"))

  lazy <- function(x) 1L
  stopifnot(identical(lazy(stop("not forced")), 1L))
  masked <- function(p, `::`) { base::length(p) }
  stopifnot(identical(masked(stop("not forced"), function(...) lazy), 1L))
})
