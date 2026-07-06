# no-diag
# A non-literal `[[` index on a list unwraps an element, but the
# selected element type is unknown; it should not be treated as list<1>.
values <- list()
key <- "duration"
out <- 1 + values[[key]]
