# no-diag
join_known_schemas <- function() {
  x <- data.frame(id = 1L, a = 2)
  y <- data.frame(id = 1L, b = "ok")
  joined <- dplyr::left_join(x, y, by = "id")
  list(joined$a + 1, paste0(joined$b, "!"))
}
