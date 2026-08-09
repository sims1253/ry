# no-diag
# `df[["Sepal.Length"]]` resolves via the iris dataset's column schema
# to the column's type (double<150>).
df <- iris
sl <- df[["Sepal.Length"]]
