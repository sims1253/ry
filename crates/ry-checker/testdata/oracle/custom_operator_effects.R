# oracle: must-pass
`+` <- function(e1, e2) { force(e1); 1L }
ignored <- assign("*", function(...) 1L) + 2L
stopifnot(identical("a" * 1L, 1L))
rm(`*`)
callee <- function() "old"
ignored <- assign("callee", function() 1L) + 2L
stopifnot(identical(callee() * 2L, 2L))
x <- "old"
if (TRUE) { ignored <- assign("x", 1L) + 2L }
stopifnot(identical(x * 2L, 2L))
x <- "old"
for (i in 1L) { ignored <- assign("x", 1L) + 2L }
stopifnot(identical(x * 2L, 2L))
