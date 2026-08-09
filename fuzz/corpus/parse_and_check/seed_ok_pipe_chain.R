# no-diag
# Chained pipes compose: c(1,2,3) %>% mean() %>% round(2) and the base-R
# equivalent both resolve to well-typed nested calls.
a <- c(1, 2, 3) %>% mean() %>% round(2)
b <- c(1, 2, 3) |> mean() |> round(2)
