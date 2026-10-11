# expect: RY040
# An own function shadows the attachment again.
paste0 <- 1L
box::use(../oracle/box_module/integer_paste[paste0])
paste0 <- function(...) "x"
paste0("x") + 1L
