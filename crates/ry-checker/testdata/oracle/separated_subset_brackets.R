# oracle: known-gap upstream tree-sitter-r requires adjacent double-subscript closing brackets
# Closing `[[` uses two independent `]` tokens in R.
x <- list(7L)
stopifnot(identical(x[[1] ], 7L))
stopifnot(identical(x[[1]
], 7L))
stopifnot(identical(x[[1] # keep this comment visible to the parser
], 7L))
y <- list(x)
stopifnot(identical(y[[1] ][[1] ], 7L))
stopifnot(identical(y[[list(1L)[[1] ]] ][[1]], 7L))

# Whitespace cannot split the opening token or repair a missing closing token.
invalid <- c("x[[1]", "x[[1] # missing bracket", "x[[1] )", "x[[1] ] ]", "x[ [1]]", "x[y[[2]", "x[[y[2]", "x[[y[2]]", "x[[1];]", "x[[1] + ]")
for (source in invalid) {
    failed <- inherits(try(parse(text = source), silent = TRUE), "try-error")
    stopifnot(failed)
}
