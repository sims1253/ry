# oracle: must-pass
stopifnot(identical(!!1, TRUE))
stopifnot(identical(rlang::inject(list(!!list(1))), list(list(1))))
result <- dplyr::mutate(data.frame(x = 1), y = !!"text")
stopifnot(identical(result$y, "text"))
wrap <- function(...) rlang::list2(...)
stopifnot(identical(wrap(!!!list(1)), list(1)))
stopifnot(identical(purrr::pluck(list(1), !!!list(1)), 1))
