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

# Literal column effects and control tags are specific to the verb.
small <- data.frame(x = 1L, g = 2L)
stopifnot(identical(names(dplyr::summarise(small, x)), "x"))
stopifnot(identical(names(dplyr::reframe(small, x)), "x"))
stopifnot(identical(names(dplyr::summarise(small, .keep = 1L)), ".keep"))
stopifnot(identical(names(dplyr::reframe(small, .groups = 1L)), ".groups"))
stopifnot(identical(names(base::transform(small, .keep = 1L)), c("x", "g", ".keep")))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, `.by` = g)), c("g", "z")))
stopifnot(identical(names(dplyr::reframe(small, z = 1L, `.by` = g)), c("g", "z")))
stopifnot(identical(names(dplyr::mutate(small, z = x + 1L, .keep = "none", .by = g)), c("g", "z")))
stopifnot(identical(names(dplyr::transmute(small, x = 2L, x = NULL)), character()))
stopifnot(identical(names(dplyr::transmute(small, x, x = NULL)), character()))
stopifnot(identical(names(dplyr::mutate(small, x, .keep = "none")), "x"))
stopifnot(identical(names(dplyr::mutate(small, x, z = g + 1L, .keep = "none")), c("x", "z")))
stopifnot(identical(names(dplyr::mutate(small, x, x = NULL, .keep = "none")), character()))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c(-x, x))), c("g", "x", "z")))
stopifnot(identical(names(dplyr::reframe(small, z = 1L, .by = c(-x, x))), c("g", "x", "z")))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = -x)), c("g", "z")))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c(x, -x))), "z"))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c(-x, x, -x))), c("g", "z")))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c(x, c(-x)))), c("x", "g", "z")))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c(c(), -x))), "z"))
stopifnot(identical(names(dplyr::mutate(small, z = 1L, .keep = "none", .by = c(x, c(-x)))), c("x", "g", "z")))
stopifnot(identical(names(dplyr::mutate(small, z = 1L, .keep = "none", .by = c(c(), -x))), "z"))
stopifnot(identical(names(dplyr::summarise(small, z = 1L, .by = c())), "z"))
stopifnot(identical(names(dplyr::mutate(small, z = 1L, .keep = "none", .by = c())), "z"))

prior_groups <- dplyr::group_by(small, g)
add <- TRUE
dynamic_add <- dplyr::group_by(prior_groups, .add = add)
stopifnot(identical(names(dplyr::summarise(dynamic_add, n = dplyr::n())), c("g", "n")))
stopifnot(identical(names(dplyr::transmute(dynamic_add, n = 1L)), c("g", "n")))
stopifnot(!inherits(dplyr::group_by(prior_groups, .add = FALSE), "grouped_df"))

rename.widget <- function(.data, ...) .data
relocate.widget <- function(.data, ...) .data
widget <- structure(data.frame(x = "a", y = 1L), class = c("widget", "data.frame"))
stopifnot(identical(dplyr::rename(widget, y = x)$y + 1L, 2L))
stopifnot(identical(dplyr::relocate(widget, y = x)$y + 1L, 2L))
left_join.widget <- function(x, y, ...) data.frame(y = 1L)
join_widget <- structure(data.frame(x = "a"), class = c("widget", "data.frame"))
stopifnot(identical(dplyr::left_join(join_widget, data.frame(x = "a"), by = "x")$y + 1L, 2L))

two_types <- data.frame(x = 1L, y = "a", g = 2L)
for (out in list(
  dplyr::rename(two_types, y = x, z = y),
  dplyr::relocate(two_types, y = x, z = y)
)) {
  stopifnot(identical(names(out), c("y", "z", "g")))
  stopifnot(is.integer(out$y), is.character(out$z))
}
stopifnot(identical(names(dplyr::relocate(two_types, g)), c("g", "x", "y")))
stopifnot(identical(class(dplyr::group_by(small, x)), c("grouped_df", "tbl_df", "tbl", "data.frame")))

# Base's declarative NSE effects also accept lists and atomic vectors.
stopifnot(identical(base::with(list(x = "a"), x), "a"))
stopifnot(identical(base::subset(c("a", "b"), TRUE), c("a", "b")))
stopifnot(identical(base::transform(list(x = "a"), y = x)$y, "a"))
stopifnot(identical(base::within(list(x = "a"), { y <- x })$y, "a"))
stopifnot(identical(base::with(list(x = 1L), x) + 1L, 2L))
stopifnot(identical(base::subset(c(1L, 2L), TRUE) + 1L, c(2L, 3L)))

# Nested tidyselect combinations are valid but are deliberately left
# incomplete by ry's bounded selector model.
stopifnot(identical(names(dplyr::select(small, c(x, c(-x)))), c("x", "g")))
stopifnot(identical(names(dplyr::select(small, c(c(), -x))), character()))
stopifnot(identical(names(dplyr::select(small, c(x, -x))), character()))

forwarded_groups <- function(...) dplyr::group_by(small, ...)
stopifnot(!inherits(forwarded_groups(), "grouped_df"))
stopifnot(inherits(forwarded_groups(x), "grouped_df"))
stopifnot(!inherits(dplyr::group_by(small, NULL), "grouped_df"))
stopifnot(!inherits(dplyr::group_by(small, c()), "grouped_df"))

# Unsupported grouping expressions may expand to no columns. A definite
# bare or computed group still selects the grouped method.
for (out in list(
  dplyr::group_by(small, !!!list()),
  dplyr::group_by(small, dplyr::across(tidyselect::starts_with("absent"))),
  dplyr::group_by(small, dplyr::pick(tidyselect::starts_with("absent"))),
  dplyr::group_by(small, if (TRUE) NULL else x)
)) {
  stopifnot(!inherits(out, "grouped_df"))
}
stopifnot(inherits(dplyr::group_by(small, x), "grouped_df"))
stopifnot(inherits(dplyr::group_by(small, big = x > mean(x)), "grouped_df"))

# A named base subset argument is a formal, not an output-column rename.
stopifnot(identical(names(base::subset(small, select = x)), "x"))
stopifnot(identical(names(base::subset(small, select = c(new = x))), "x"))
stopifnot(identical(base::with(base::subset(small, select = x), x + 1L), 2L))
