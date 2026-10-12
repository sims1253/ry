# no-diag
# Non-function writes keep calls through an attachment opaque.
paste0 <- 1L
box::use(../oracle/box_module/integer_paste[paste0])
paste0 <- 2L
paste0("x") + 1L
for (i in 1:2) {}
paste0("x") + 1L
widen <- function() paste0 <<- 2L
widen()
paste0("x") + 1L
