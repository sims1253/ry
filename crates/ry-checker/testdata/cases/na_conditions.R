## case: a literal NA if condition always errors
# expect: RY001
if (NA) 1L else 2L

## case: a literal NA while condition always errors
# expect: RY001
while (NA) break

## case: typed NA literals are proven NA conditions
# expect: RY001 RY001 RY001 RY001
if (NA_integer_) 1L else 2L
if (NA_real_) 1L else 2L
if (NA_character_) 1L else 2L
if (NA_complex_) 1L else 2L

## case: a comparison between a typed NA and a plain literal is NA
# expect: RY001
if (NA_integer_ > 1L) 1L else 2L

## case: negating a literal NA keeps it NA
# expect: RY001
if (!NA) 1L else 2L

## case: arithmetic between a literal NA and a plain literal is NA
# expect: RY001
if (NA + 1L) 1L else 2L

## case: any over a literal NA is NA
# expect: RY001
if (any(NA)) 1L else 2L

## case: sum over a typed NA is NA
# expect: RY001
if (sum(NA_integer_)) 1L else 2L

## case: a definite RY119 cast is an NA condition as well as a range loss
# expect: RY001 RY119
if (as.integer(1e10)) 1L else 2L

## case: an is.na guard is the idiom and stays silent
# no-diag
check_value <- function(x) {
  if (!is.na(x)) 1L else 2L
}
check_value(1L)

## case: isTRUE absorbs a possible NA and stays silent
# no-diag
check_flag <- function(x) {
  if (isTRUE(x)) 1L else 2L
}
check_flag(NA)

## case: a binding to NA is not proof, because replacement can repair it
# no-diag
x <- NA
x[1L] <- TRUE
if (x) 1L else 2L

## case: NaN compared as a string is not an NA proof
# no-diag
if (NaN == "NaN") 1L else 2L

## case: any with na.rm removes the NA instead of propagating it
# no-diag
if (any(NA, na.rm = TRUE)) 1L else 2L
