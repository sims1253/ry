# oracle: must-pass
# if() with a numeric condition coerces to logical in R (nonzero = TRUE).
x <- if (1) 10 else 20
