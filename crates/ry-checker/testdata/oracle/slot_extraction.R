# oracle: must-pass
x <- 1:3
stopifnot(identical(x@.Data, 1:3))
x <- factor('a')
stopifnot(identical(x@.Data, 1L))
local({
  `@` <- function(object, name) { assign('y', 42L, parent.frame()); 7L }
  y <- list(1L)
  stopifnot(identical(1L@anything, 7L), identical(y, 42L))
})
local({
  `@<-` <- function(object, name, value) { assign('y', 42L, parent.frame()); 9L }
  x <- list(a=list(b=1L)); y <- list(1L)
  x@anything <- 2L
  stopifnot(identical(x, 9L), identical(y, 42L), identical(x[1], 9L))
})
local({
  `@` <- function(object, name) list(bar=list(baz=1L))
  `@<-` <- function(object, name, value) 11L
  x <- list(old=1L)
  x@foo$bar$baz <- 2L
  stopifnot(identical(x, 11L), identical(x[1], 11L))
})
local({
  `@<-` <- function(object, name, value) 12L
  x <- list(foo=list(old=1L))
  x$foo@bar <- 2L
  stopifnot(identical(x$foo, 12L))
})
local({
  methods::setClass('SlotReview', slots=c(foo='numeric'))
  x <- methods::new('SlotReview', foo=1)
  stopifnot(identical(x@foo, 1))
  stopifnot(inherits(tryCatch(x$foo, error=identity), 'error'))
  stopifnot(inherits(tryCatch(x@foo <- 'bad', error=identity), 'error'))
  stopifnot(inherits(tryCatch(1L@foo, error=identity), 'error'))
})
