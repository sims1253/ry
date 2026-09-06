# no-diag
`+.left` <- function(e1, e2) "left"
`+.right` <- function(e1, e2) "right"
chooseOpsMethod.left <- function(x, y, mx, my, cl, reverse) TRUE
x <- structure(data.frame(a = 1), class = c("left", "data.frame"))
y <- structure(2, class = "right")
# Custom selection can return a string, so no frame schema is certain.
result <- x + y
result == "left"
reversed <- y + x
reversed == "left"
d <- data.frame(a = 1, b = 2)
shifted <- d + 1
shifted$a
