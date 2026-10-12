# no-diag
# A call skips the own value and may reach the attachment.
foo <- 1L
box::use(./modules/text_function[foo])
foo() + 1L
