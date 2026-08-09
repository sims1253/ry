# no-diag
# `mtcars` has class "data.frame" (via the typeshed), and `print.data.frame`
# is registered in the typeshed's S3 method table, so `print(df)` resolves
# without RY050.
df <- mtcars
print(df)
