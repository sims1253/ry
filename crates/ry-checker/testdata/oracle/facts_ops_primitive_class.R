# oracle: must-pass
# `ry dump-facts` classes for arithmetic with no `Ops` method, checked
# against GNU R by the ry-cli facts oracle (#865). R's primitive keeps the
# longer operand's class. At equal lengths the left class wins and a
# classless left keeps the right class; comparisons and empty results drop it.
# oracle-facts: scaled flipped left longer dropped empty compared plain
scaled <- structure(1, class = "foo") * 2
flipped <- 2 * structure(1, class = "foo")
left <- structure(1, class = "foo") + structure(1, class = "bar")
longer <- structure(1, class = "bar") + structure(c(1, 2), class = "foo")
dropped <- structure(1, class = "foo") + c(1, 2, 3)
empty <- structure(numeric(0), class = "foo") + 1
compared <- structure(1, class = "foo") == 2
plain <- 1 + 2
