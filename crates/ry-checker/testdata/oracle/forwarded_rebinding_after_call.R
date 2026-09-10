# oracle: must-flag
callee <- function(bins = 30L) if (bins == 1L) 1L else 2L
caller <- function(`bins` = NULL) {
  identity(callee(bins))
  bins <- 1L
}
caller()
