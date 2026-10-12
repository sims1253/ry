# no-diag
# An attachment does not hide the caller's own binding.
foo <- 1L
box::use(./modules/text_values[foo])
foo + 1L
box::use(./modules/text_values[...])
foo + 1L
