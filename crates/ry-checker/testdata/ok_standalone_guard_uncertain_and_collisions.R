# no-diag
maybe_string <- unknown_value()
check_string(maybe_string)

nullable <- NULL
check_string(nullable, allow_null = TRUE)

check_string <- function(x, ...) TRUE
value <- 1L
check_string(value)
