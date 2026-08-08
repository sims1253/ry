# oracle: must-pass
# oracle-claim: RY102
# `<-` inside list() creates an unnamed element and a binding as a side effect.
make_value <- function() {
  value <- list(accidental <- 2L)
  stopifnot(is.null(names(value)), identical(accidental, 2L))
}
make_value()
