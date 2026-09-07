# oracle: must-pass
`+` <- function(e1, e2) {
    assign("x", list(field = 1L), envir = parent.frame())
    NULL
}
f <- function(flag) {
    x <- 1L
    while (TRUE) {
        if (flag) break
        1 + 2
        break
    }
    if (!flag) x$field
}
stopifnot(identical(f(FALSE), 1L), is.null(f(TRUE)))
