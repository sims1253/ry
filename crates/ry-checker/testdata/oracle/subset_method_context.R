# oracle: must-pass
# R supplies these bindings to subset and subset replacement methods.
`[.widget` <- function(x, ...) {
  stopifnot(.Generic == "[", .Method == "[.widget", identical(.Class, "widget"))
  unclass(x)
}
`[[.widget` <- function(x, ...) {
  stopifnot(.Generic == "[[", .Method == "[[.widget", identical(.Class, "widget"))
  unclass(x)
}
`$.widget` <- function(x, ...) {
  stopifnot(.Generic == "$", .Method == "$.widget", identical(.Class, "widget"))
  unclass(x)
}
`[<-.widget` <- function(x, ...) {
  stopifnot(.Generic == "[<-", .Method == "[<-.widget", identical(.Class, "widget"))
  unclass(x)
}
`[[<-.widget` <- function(x, ...) {
  stopifnot(.Generic == "[[<-", .Method == "[[<-.widget", identical(.Class, "widget"))
  unclass(x)
}
`$<-.widget` <- function(x, ...) {
  stopifnot(.Generic == "$<-", .Method == "$<-.widget", identical(.Class, "widget"))
  unclass(x)
}
x <- structure(list(a = 1L), class = "widget")
x[1]
x <- structure(list(a = 1L), class = "widget")
x[[1]]
x <- structure(list(a = 1L), class = "widget")
x$a
x <- structure(list(a = 1L), class = "widget")
x[1] <- list(2L)
x <- structure(list(a = 1L), class = "widget")
x[[1]] <- 2L
x <- structure(list(a = 1L), class = "widget")
x$a <- 2L
