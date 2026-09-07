# oracle: must-pass
as.list.ry_callback_empty <- function(x, ...) list()
x <- structure("a", class = "ry_callback_empty")
mapped <- lapply(x, function(v) 1L)
typed <- vapply(x, function(v) 1L, integer(1), USE.NAMES = FALSE)
simplified <- sapply(x, function(v) 1L, USE.NAMES = FALSE)
stopifnot(identical(mapped, list()), identical(typed, integer()),
          identical(simplified, list()))
length.ry_callback_zero <- function(x) 0L
y <- structure("a", class = "ry_callback_zero")
multiple <- mapply(function(v) 1L, y, USE.NAMES = FALSE)
stopifnot(identical(multiple$field, NULL))
