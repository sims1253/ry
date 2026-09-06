# oracle: must-flag
capture <- function(x) substitute(x)
forward <- function(x) capture(x)
forward <- function(x) x
forward(1 + "a")
