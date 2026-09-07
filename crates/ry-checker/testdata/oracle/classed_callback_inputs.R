# oracle: must-pass
x <- structure(c("a", "b"), class = "ry_callback_input")
as.list.ry_callback_input <- function(x, ...) list(1L, 2L)
`[[.ry_callback_input` <- function(x, i, ...) 1L
filtered <- Filter(function(v) v + 1L > 0L, x)
position <- Position(function(v) v + 1L > 0L, x, right = TRUE)
found <- Find(function(v) v + 1L > 0L, x)
mapped <- lapply(x, function(v) v + 1L)
stopifnot(length(filtered) == 2L, identical(position, 2L),
          identical(found, 1L), identical(mapped, list(2L, 3L)))
