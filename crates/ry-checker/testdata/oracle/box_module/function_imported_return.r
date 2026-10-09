foo <- function(x = 1L) {
  box::use(./integer_paste[paste0])
  paste0("x")
}
box::export(foo)
