# oracle: must-pass
callee <- function(bins = 30L) if (bins == 1L) 1L else 2L
assigned <- function(bins = NULL) {
  bins[1L] <- 1L
  value <- callee(bins)
  value
}
wrapped <- function(bins = NULL) {
  bins[[1L]] <- 1L
  identity(callee(bins))
}
returned <- function(`bins` = NULL) {
  assign("bins", 1L)
  return(callee(`bins` = bins))
}
delayed <- function(bins = NULL) {
  delayedAssign("bins", 1L)
  callee(bins)
}
looped <- function(bins = NULL) {
  for (bins in list(1L)) callee(bins)
}
stopifnot(assigned() == 1L, wrapped() == 1L, returned() == 1L, delayed() == 1L)
looped()
invoke <- function(callback = NULL) callback("ok")
visit <- function(callback = NULL) {
  callback <- function(value) value
  for (i in 1:2) invoke(callback)
}
visit()
bins <- 0L
local_rhs <- function(bins = NULL) {
  bins <<- (bins <- 1L)
  callee(bins)
}
right_local_rhs <- function(bins = NULL) {
  (bins <- 1L) ->> bins
  callee(bins)
}
local_outer <- function(bins = NULL) {
  bins <- (bins <<- 1L)
  callee(bins)
}
right_local_outer <- function(bins = NULL) {
  bins <- (1L ->> bins)
  callee(bins)
}
stopifnot(local_rhs() == 1L, right_local_rhs() == 1L,
          local_outer() == 1L, right_local_outer() == 1L)
