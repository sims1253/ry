# no-diag
# A character `:` endpoint works only when its text parses as a finite
# number: `"1":"3"` is 1:3, while `1:"a"` errors at the `:` itself
# ("NA/NaN argument") and `1:"Inf"` is too long a vector. ry cannot read the
# text statically, so the result stays unknown and the later arithmetic,
# which never runs, is not reported (#648).
x <- 1:"a"
y <- x + "a"
s <- "a"
z <- 1:s
w <- z + "a"
v <- 1:"Inf"
u <- v + "a"
