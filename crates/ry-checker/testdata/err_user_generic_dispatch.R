# expect: RY040, RY050
# A project `UseMethod` generic dispatches on its first argument's class,
# whatever the method's first formal is called: the selected method's
# return type flows out of the call, and a fully known class with neither
# a method nor a default is reported.
lbl <- function(x, ...) UseMethod("lbl")
lbl.circle <- function(shape, ...) "a circle"
lbl.rect <- function(obj, ...) "a rect"
lbl.square <- function(obj, ...) paste("square,", NextMethod())
circle <- structure(list(), class = "circle")
square <- structure(list(), class = c("square", "rect"))
pillar <- structure(list(), class = c("pillar", "rect"))
triangle <- structure(list(), class = "triangle")
lbl(circle) + 1
lbl(square)
lbl(pillar)
lbl(triangle)
