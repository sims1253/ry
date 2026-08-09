# no-diag
# Built-in dataset `mtcars` resolves to a list-typed value from the
# typeshed's datasets table, so no RY010 (unbound variable) is emitted.
df <- mtcars
