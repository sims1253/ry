# oracle: must-pass
# Without an `Ops` method, R's arithmetic primitive keeps the class of the
# longer operand. At equal lengths the left class wins and a classless left
# keeps the right class; comparisons and empty results drop it (#865). The
# retained class makes `$` dispatch to `$.foo`, so `$` here is not the
# atomic-vector error.
`$.foo` <- function(x, name) paste("foo", name)
scaled <- structure(1, class = "foo") * 2
stopifnot(identical(class(scaled), "foo"), identical(scaled$a, "foo a"))
flipped <- 2 * structure(1, class = "foo")
stopifnot(identical(class(flipped), "foo"), identical(flipped$b, "foo b"))
left <- structure(1, class = "foo") + structure(1, class = "bar")
stopifnot(identical(class(left), "foo"), identical(left$c, "foo c"))
longer <- structure(1, class = "bar") + structure(c(1, 2), class = "foo")
stopifnot(identical(class(longer), "foo"), identical(longer$d, "foo d"))
stopifnot(is.null(oldClass(structure(1, class = "foo") + c(1, 2, 3))))
stopifnot(is.null(oldClass(structure(numeric(0), class = "foo") + 1)))
stopifnot(is.null(oldClass(structure(1, class = "foo") == 2)))
# A custom `Ops` method decides the class instead of the primitive.
Ops.qux <- function(e1, e2) {
  structure(get(.Generic)(unclass(e1), unclass(e2)), class = "foo")
}
custom <- structure(1, class = "qux") * 2
stopifnot(identical(class(custom), "foo"), identical(custom$e, "foo e"))
