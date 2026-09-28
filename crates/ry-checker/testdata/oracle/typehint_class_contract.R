# oracle: must-pass
# oracle-claim: RY114
# The audited provider compares class(), not typeof(). These assertions pin
# the mismatch premise without loading or executing the provider package.
stopifnot(identical(class(1L), "integer"))
stopifnot(identical(class(1), "numeric"))
stopifnot(identical(class(structure(1.0, class = "integer")), "integer"))
stopifnot(identical(class(structure("x", class = "integer")), "integer"))
stopifnot(identical(class(matrix(1L)), c("matrix", "array")))
stopifnot(identical(class(structure(1L, class = c("a", "b"))), c("a", "b")))
