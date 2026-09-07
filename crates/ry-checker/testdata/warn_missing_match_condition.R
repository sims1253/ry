# expect: RY003
# A missing match returns NA_integer_, so this scalar integer condition
# errors in R. It must not inherit the never-NA non-empty count idiom.
if (match(1L, 2L)) print(1)
