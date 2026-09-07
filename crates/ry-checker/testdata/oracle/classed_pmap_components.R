# oracle: must-pass
vec_proxy.ry_callback_component <- function(x, ...) list(1L)
vec_restore.ry_callback_component <- function(x, to, ...) x
x <- structure(list("a"), class = "ry_callback_component")
result <- purrr::pmap(list(x, 1:2), function(v, other) v + other)
stopifnot(identical(result, list(2L, 3L)))
