# oracle: must-pass
x <- purrr::map_if(list(1, 2), c(FALSE, TRUE), function(x) "text")
stopifnot(identical(x[[1]] + 1, 2))
y <- purrr::accumulate(1:3, function(x, y) x + y)
stopifnot(identical(y, c(1L, 3L, 6L)))
