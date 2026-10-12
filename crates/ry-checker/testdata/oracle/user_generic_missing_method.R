# oracle: must-warn RY050
# oracle-claim: RY050
# A project generic finds methods whatever their first formal is called,
# walks the class vector (and NextMethod) in order, and errors in R when a
# fully known class has neither a method nor a default.
lbl <- function(x, ...) UseMethod("lbl")
lbl.circle <- function(shape, ...) "a circle"
lbl.rect <- function(obj, ...) "a rect"
lbl.square <- function(obj, ...) paste("square,", NextMethod())
stopifnot(identical(lbl(structure(list(), class = "circle")), "a circle"))
stopifnot(identical(lbl(structure(list(), class = c("square", "rect"))), "square, a rect"))
stopifnot(identical(lbl(structure(list(), class = c("pillar", "rect"))), "a rect"))
error <- tryCatch(lbl(structure(list(), class = "triangle")), error = identity)
stopifnot(inherits(error, "error"))
