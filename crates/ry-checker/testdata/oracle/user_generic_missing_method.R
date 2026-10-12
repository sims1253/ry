# oracle: must-warn RY050
# oracle-claim: RY050
# A project generic finds methods whatever their first formal is called,
# walks the class vector (and NextMethod) in order, and errors in R when a
# fully known class has neither a method nor a default. Dispatch follows the
# actual partially matched to the first formal, and a method bound in the
# calling environment wins over the global one.
lbl <- function(x, ...) UseMethod("lbl")
lbl.circle <- function(shape, ...) "a circle"
lbl.rect <- function(obj, ...) "a rect"
lbl.square <- function(obj, ...) paste("square,", NextMethod())
stopifnot(identical(lbl(structure(list(), class = "circle")), "a circle"))
stopifnot(identical(lbl(structure(list(), class = c("square", "rect"))), "square, a rect"))
stopifnot(identical(lbl(structure(list(), class = c("pillar", "rect"))), "a rect"))
error <- tryCatch(lbl(structure(list(), class = "triangle")), error = identity)
stopifnot(inherits(error, "error"))
area <- function(object, other) UseMethod("area")
area.foo <- function(object, other) "foo"
area.bar <- function(object, other) "bar"
stopifnot(identical(area(structure(1, class = "foo"), obj = structure(1, class = "bar")), "bar"))
local_lbl <- function() {
  lbl.circle <- function(x, ...) "local circle"
  lbl(structure(list(), class = "circle"))
}
stopifnot(identical(local_lbl(), "local circle"))
