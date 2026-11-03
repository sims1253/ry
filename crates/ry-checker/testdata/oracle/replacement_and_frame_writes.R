# oracle: must-pass
# Writes ry must not ignore when it exports scope-exit facts (#862, #867).
# An unaudited replacement function replaces its target's mode, length, or
# class; base `length<-` keeps a classless vector's mode.
x <- 1L; `foo<-` <- function(x, value) "s"; foo(x) <- 1
stopifnot(is.character(x))
y <- 1:3; length(y) <- 5
stopifnot(is.integer(y), length(y) == 5L)
h <- 1; oldClass(h) <- "myc"
k <- 1; attributes(k) <- list(class = "myc")
l <- 1; mostattributes(l) <- list(class = "myc")
stopifnot(inherits(h, "myc"), inherits(k, "myc"), inherits(l, "myc"))
# Frame writes inside a call: each returns or leaves `x` character, and
# `rm()` removes the binding.
f <- function() { x <- 1L; eval(quote(x <- "a")); x }
e <- function() { x <- 1L; evalq(x <- "a"); x }
g <- function() { x <- 1L; assign("x", "a", envir = environment()); x }
g2 <- function() {
  x <- 1L
  assign(envir = environment(), x = "x", value = "a", inherits = FALSE)
  x
}
r <- function() { x <- 1L; rm(x); exists("x", inherits = FALSE) }
r2 <- function() { x <- 1L; rm(x, inherits = FALSE); exists("x", inherits = FALSE) }
exit <- NULL
w <- function() { x <- 1L; on.exit(x <- "a"); on.exit(exit <<- x, add = TRUE); x }
stopifnot(identical(f(), "a"), identical(e(), "a"), identical(g(), "a"))
stopifnot(identical(g2(), "a"), !r(), !r2(), identical(w(), 1L), identical(exit, "a"))
