# no-diag
# Type narrowing: `is.character(x)` narrows `x` to character inside
# the then branch. `nchar` and `toupper` on character are well-typed.
x <- eval(parse(text = "\"hello\""))
if (is.character(x)) {
  n <- nchar(x)
  u <- toupper(x)
}
