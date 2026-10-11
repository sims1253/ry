# no-diag
# Shared non-key columns come back as `v.x` / `v.y`; a natural join keys on them.
left <- data.frame(id = 1L, v = 2)
right <- data.frame(id = 1L, v = "a")
joined <- dplyr::left_join(left, right, by = "id")
joined$v.x + 1
paste0(joined$v.y, "!")
natural <- dplyr::inner_join(left, data.frame(id = 2L, w = TRUE))
natural$w & TRUE
# Named tables can surround the `by` argument.
reordered <- dplyr::left_join(x = left, by = "id", y = right)
reordered$v.x + 1
