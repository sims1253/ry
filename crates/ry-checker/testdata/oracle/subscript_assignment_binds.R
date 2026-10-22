# oracle: must-pass
# `[` is a primitive: it evaluates its subscripts eagerly in the calling
# frame, so an assignment inside one binds there whatever the receiver
# (#713). lme4's mlist2vec and DescTools' PlotBag rely on it.
ff <- function(x) {
  if (all(x[iu <- upper.tri(x)] == 0)) t(x[!iu]) else t(x)[!iu]
}
stopifnot(identical(ff(matrix(c(1, 2, 0, 4), 2)), t(c(1, 2, 4))))

loop_points <- function(five) {
  list(five[h <- c(1, 5), 1], five[h, 2])
}
five <- matrix(1:10, 5)
stopifnot(identical(loop_points(five), list(c(1L, 5L), c(6L, 10L))))

m <- matrix(1:4, 2)
z <- m[j <- 1, 2]
stopifnot(identical(j, 1), identical(z, 3L))

opaque <- function(x) {
  x[[n <- 1]]
  n
}
stopifnot(identical(opaque(list(7)), 1))
