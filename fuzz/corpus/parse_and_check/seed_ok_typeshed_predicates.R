# no-diag
# Type predicates from the expanded typeshed: `is.numeric`, `is.character`,
# `is.integer`, `is.logical`, `is.null`, `is.list`, `is.matrix`,
# `is.data.frame`, `is.factor` all return logical<1>. Using them in an
# `if` condition is well-typed.
x <- c(1, 2, 3)
if (is.numeric(x)) print(x)
if (is.character("hello")) print("yes")
if (!is.null(x)) print("not null")
if (is.list(list())) print("is list")
