# oracle: must-pass
sprintf <- function(...) 1L
stopifnot(identical(sprintf('%d'), 1L))
gettextf <- function(...) 2L
stopifnot(identical(gettextf('%s'), 2L))

local({
  sprintf <- function(...) 1L
  probe <- sprintf
  stopifnot(identical(probe('%d'), 1L))
})
