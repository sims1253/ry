# oracle: must-pass
# dplyr d5e94e7, tests/testthat/test-group-by.R:17-44 computes `big`.
d <- data.frame(x = 1:4, g = rep(1:2, each = 2))
grouped <- d |>
  dplyr::group_by(g) |>
  dplyr::group_by(big = x > mean(x), .add = TRUE)
stopifnot(identical(grouped$big, c(FALSE, FALSE, TRUE, TRUE)))
stopifnot(identical(names(grouped), c("x", "g", "big")))

mutated <- dplyr::mutate(d, new = x + 1L)
dropped <- dplyr::mutate(d, x = NULL)
kept_none <- dplyr::mutate(d, new = x + 1L, .keep = "none")
transmuted <- dplyr::transmute(d, new = x + 1L)
renamed <- dplyr::rename(d, new = x)
relocated <- dplyr::relocate(d, new = x)
selected <- dplyr::select(d, new = x)
summarised <- dplyr::summarise(d, n = dplyr::n(), .by = g)

stopifnot(identical(names(mutated), c("x", "g", "new")))
stopifnot(identical(names(dropped), "g"))
stopifnot(identical(names(kept_none), "new"))
stopifnot(identical(names(transmuted), "new"))
stopifnot(identical(names(renamed), c("new", "g")))
stopifnot(identical(names(relocated), c("new", "g")))
stopifnot(identical(names(selected), "new"))
stopifnot(identical(names(summarised), c("g", "n")))
