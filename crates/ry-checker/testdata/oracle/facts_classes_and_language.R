# oracle: must-pass
# `ry dump-facts` claims for classed values, lists, functions, and
# language objects, checked against GNU R by the ry-cli facts oracle (#858).
# oracle-facts: tagged frame fac pair closure call symbol formula
tagged <- structure(1:2, class = c("foo", "bar"))
frame <- data.frame(a = 1:3)
fac <- factor(c("a", "b"))
pair <- list(1, "a")
closure <- function(x) x + 1
call <- quote(a + b)
symbol <- quote(a)
formula <- y ~ x
