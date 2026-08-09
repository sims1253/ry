# no-diag
# `mtcars` has a `columns` schema in the typeshed, so `df$mpg` resolves
# to the column's type (double<32>) without any diagnostic.
df <- mtcars
x <- df$mpg
