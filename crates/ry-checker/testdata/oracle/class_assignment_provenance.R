# oracle: must-pass
x <- 1L
c <- function(...) "actual"
class(x) <- c("widget")
stopifnot(identical(class(x), "actual"))
y <- 1L
class(y) <- base::c("widget", recursive = "TRUE")
stopifnot(identical(class(y), "widget"))
z <- 1L
class(z) <- { `class<-` <- function(x, value) "custom"; "widget" }
stopifnot(identical(z, "custom"))
rm(`class<-`)
c <- function(...) "character"
a <- 1L
class(a) <- c("widget")
stopifnot(identical(a, "1"))
b <- 1L
class(b) <- base::c("double", "widget")
stopifnot(typeof(b) == "integer", identical(class(b), base::c("double", "widget")))
record <- structure(list(a = 1L), class = "widget")
class(record) <- NULL
stopifnot(identical(record, list(a = 1L)))
masked <- with(list(`class<-` = function(x, value) "custom"), {
  x <- 1L
  class(x) <- "widget"
  x
})
stopifnot(identical(masked, "custom"))
