# oracle: must-pass
# Forwarded dots expand into actual arguments, even if the dots expression is named.
attr2 <- function(...) base::attr(..., exact = TRUE)
x <- structure(1L, label = 'ok')
stopifnot(identical(attr2(x, 'label'), 'ok'))
gsubi <- function(...) gsub(..., ignore.case = TRUE)
stopifnot(identical(gsubi('a', 'b', 'A'), 'b'))
replace <- function(x, ...) gsub(x = x, ...)
stopifnot(identical(replace('A', 'a', 'b', ignore.case = TRUE), 'b'))
target <- function(x, y) x + y
forward <- function(...) target(tag = ...)
stopifnot(identical(forward(1L, 2L), 3L))
quoted_forward <- function(...) target(`...`)
stopifnot(identical(quoted_forward(1L, 2L), 3L))

# Expanded exact names can disambiguate an otherwise ambiguous partial match.
target <- function(alpha, alpine) alpha + alpine
partial <- function(...) target(`al` = 1L, ...)
stopifnot(identical(partial(alpine = 2L), 3L))
# A tag on dots does not reserve the same-named formal.
target <- function(xyz, y) xyz + y
partial <- function(...) target(xyz = ..., x = 1L)
stopifnot(identical(partial(y = 2L), 3L))
