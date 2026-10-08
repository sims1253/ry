#' @export

blank <- 1L
#' @export
# An ordinary comment remains in this declaration's comment region.
ordinary <- 2L
##' @export
hashes <- 3L
#' @export

# Reexported names obey the same comment-region rules.
box::use(./hello[attached = foo])
hidden <- 4L
