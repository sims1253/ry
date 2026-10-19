# oracle: must-pass
# Actuals a one-operation body accepts or dispatches on stay quiet.
f <- function(x) x + 1L
nm <- function(df) df$value * 2
get_value <- function(df) df$value
id <- function(x) x
Ops.myop <- function(e1, e2) "ok"
stopifnot(
  identical(f(2L), 3L),
  identical(f(TRUE), 2L),
  identical(f(structure("a", class = "myop")), "ok"),
  identical(nm(list(value = 1)), 2),
  identical(nm(NULL), numeric(0)),
  identical(nm(data.frame(value = 1)), 2),
  is.null(get_value(NULL)),
  identical(id(1L) + 1L, 2L),
  identical(f(x = 1L), 2L)
)
g <- function(x, ...) x
stopifnot(identical(g(1L, "a") + 1L, 2L))
