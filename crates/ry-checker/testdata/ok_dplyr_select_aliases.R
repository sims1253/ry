# no-diag
# Each named selector of the same column is its own output column.
wide <- data.frame(x = 1L, y = 2L, g = 3L)
both <- dplyr::select(wide, a = x, b = x)
both$a + both$b
renamed <- dplyr::select(wide, -x, a = y, b = y)
renamed$a + renamed$g + renamed$b
