# oracle: must-pass
# Replacement assignment changes the receiver but evaluates to the RHS.
x <- "2"
storage.mode(x) <- "integer"
stopifnot(identical(x + 1L, 3L))
x <- list(2)
storage.mode(x) <- "integer"
stopifnot(identical(x + 1L, 3L))
x <- "2"
mode(x) <- "numeric"
stopifnot(identical(x + 1, 3))
x <- 2L
assigned <- (storage.mode(x) <- "character")
stopifnot(identical(x, "2"), identical(assigned, "character"))
convert <- function(mode = "integer") {
  value <- "2"
  storage.mode(value) <- mode
  value
}
stopifnot(identical(convert() + 1L, 3L))
local({
  `storage.mode<-` <- function(x, value) 2L
  x <- "old"
  storage.mode(x) <- "integer"
  stopifnot(identical(x + 1L, 3L))
})
local({
  `mode<-` <- function(x, value) 2L
  x <- "old"
  mode(x) <- "integer"
  stopifnot(identical(x + 1L, 3L))
})

# A custom replacement may preserve a list or replace it with an atom.
# Its spelling alone cannot prove the result still has list storage.
local({
  `mode<-` <- function(x, value) 1L
  x <- list(1L)
  mode(x) <- "integer"
  stopifnot(identical(x[1L], 1L))
})
local({
  `storage.mode<-` <- function(x, value) 1L
  x <- list(1L)
  storage.mode(x) <- "integer"
  stopifnot(identical(x[1L], 1L))
})
local({
  `mode<-` <- function(x, value) x
  x <- list(1L)
  mode(x) <- "integer"
  stopifnot(is.list(x), !identical(x[1L], 1L))
})
local({
  `storage.mode<-` <- function(x, value) x
  x <- list(1L)
  storage.mode(x) <- "integer"
  stopifnot(is.list(x), !identical(x[1L], 1L))
})
