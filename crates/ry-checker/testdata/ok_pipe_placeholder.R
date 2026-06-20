# no-diag
# Magrittr placeholder: the first `.` is substituted with the LHS value,
# so `round(., digits = 2)` becomes `round(c(1,2,3), digits = 2)`.
result <- c(1, 2, 3) %>% round(., digits = 2)
