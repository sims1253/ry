# oracle: must-pass
# Indexed replacement writes coerce and extend the way R does (#839), and
# `$<-` on an atomic vector only warns and coerces the LHS to a list rather
# than raising the read-side `$` error (#840). R 4.5.0 verified below.
ch <- character(0); ch[["a"]] <- "A"
stopifnot(identical(ch, c(a = "A")))
nl <- NULL; nl[[1]] <- 1:3
stopifnot(identical(nl, list(1:3)))
x <- 1:3; x[5] <- 2.5
stopifnot(identical(x, c(1, 2, 3, NA, 2.5)))
r <- integer(0); for (i in 1:5) r[i] <- i
stopifnot(identical(r, 1:5))
v <- 1:3; v[[2]] <- list(1)
stopifnot(identical(v, list(1L, list(1), 3L)))
w <- 1:3; w$field <- 5L
stopifnot(identical(w, list(1L, 2L, 3L, field = 5L)))
# A positional write keeps names, so the later named write replaces an
# element instead of adding one.
y <- c(a = 1L); y[1] <- 2L; y[["a"]] <- 3L
stopifnot(identical(y, c(a = 3L)))
# `$<- NULL` deletes after coercing to a list; a missing name adds nothing.
d <- 1:3; d$a <- NULL
stopifnot(identical(d, list(1L, 2L, 3L)))
e <- c(a = 1L, b = 2L); e$a <- NULL
stopifnot(identical(e, list(b = 2L)))
# A replacement that may be NULL either deletes the element or replaces it.
pick <- function(flag) {
  x <- c(a = 1L)
  v <- if (flag) NULL else 2L
  x$a <- v
  length(x)
}
stopifnot(identical(pick(TRUE), 0L), identical(pick(FALSE), 1L))
pick_absent <- function(flag) {
  x <- c(a = 1L)
  v <- if (flag) NULL else 2L
  x$b <- v
  length(x)
}
stopifnot(identical(pick_absent(TRUE), 1L), identical(pick_absent(FALSE), 2L))
# An `is.na()` repair coerces the mode like any other subassignment.
r <- c(1, NA); r[is.na(r)] <- "none"
stopifnot(identical(r, c("1", "none")))
# A definite NULL written into a NULL base stays NULL.
n <- NULL; n[[1]] <- NULL
stopifnot(is.null(n))
# A definite NULL written into a fresh NULL base stays NULL, including the
# `$<-` spelling R 4.5.0; only an already-list LHS becomes an empty list.
n2 <- NULL; n2$a <- NULL
stopifnot(is.null(n2))
n3 <- NULL; n3[1] <- NULL
stopifnot(is.null(n3))
# `[[<-` extends an existing vector too: out-of-range positions are
# padded, for atomic and list receivers alike (R 4.5.0).
ext <- 1:3; ext[[5]] <- 1L
stopifnot(identical(ext, c(1L, 2L, 3L, NA, 1L)))
lst <- list(1); lst[[5]] <- 2
stopifnot(length(lst) == 5L, is.null(lst[[4]]))

# Repeated and empty name subscripts: non-empty names dedup to their first
# occurrence on lists, the empty name adds every time, and data frames give
# each occurrence a column.
l1 <- list(a = 1L); l1[c("b", "b")] <- 2L
stopifnot(length(l1) == 2L)
l2 <- list(a = 1L); l2[c("", "")] <- 1:2
stopifnot(length(l2) == 3L)
l3 <- list(a = 1L); l3[c("b", "", "b")] <- 1:3
stopifnot(length(l3) == 3L)
df1 <- data.frame(a = 1L); df1[c("b", "b")] <- 7
stopifnot(ncol(df1) == 3L)
