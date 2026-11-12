# oracle: must-pass
# return() inside evalq() or local(envir = environment()) returns from the
# eval call, not from the enclosing function, which keeps running (#897).
f <- function() { x <- 1L; local({ y <- "a"; return(y); NULL }, envir = environment()); x <- TRUE; x }
g <- function() { x <- 1L; evalq({ return("a") }); x <- TRUE; x }
stopifnot(isTRUE(f()), isTRUE(g()))
