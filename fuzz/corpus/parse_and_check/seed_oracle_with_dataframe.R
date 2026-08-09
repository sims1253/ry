# oracle: must-pass
# with() evaluates an expression in the scope of a list/data frame; R succeeds.
df <- list(a = 1, b = 2)
v <- with(df, a + b)
print(v)
