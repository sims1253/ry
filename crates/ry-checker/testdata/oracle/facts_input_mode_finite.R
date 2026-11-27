# oracle: must-pass
# oracle-facts: range_empty_finite_false range_na_finite_true range_int_finite_false
# `range()` excludes `finite` from data arguments and treats `finite = TRUE`
# as able to drop elements (#922).
range_empty_finite_false <- range(integer(0), finite = FALSE)
range_na_finite_true <- range(NA_integer_, finite = TRUE)
range_int_finite_false <- range(1L, finite = FALSE)
