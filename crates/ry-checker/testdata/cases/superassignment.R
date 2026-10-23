# A closure body runs when it is called, not where it is defined, so a
# `<<-` writer defined after the reader may already have run (issue #706).

## case: a closure reads a `<<-` target whose writer is defined after it
# no-diag
spin_control <- function() {
  pressed <- NULL
  rotate <- function() if (pressed) "rotating"
  on_press <- function() pressed <<- TRUE
  list(rotate = rotate, on_press = on_press)
}
helper_late <- NULL
run_late <- function(x) helper_late(x)
api <- local({
  helper_late <<- function(x) x * 2
})

## case: a later writer that cannot reach the binding keeps the closure's stale type
# expect: RY001, RY001
# The first writer targets another name. The second lands in `outer`'s
# formal, never the file-level `x`.
spin_other <- function() {
  pressed <- NULL
  rotate <- function() if (pressed) "rotating"
  on_press <- function() other <<- TRUE
}
x <- NULL
reader <- function() if (x) 1
outer <- function(x) {
  inner <- function() x <<- TRUE
}

## case: an eager read before a later writer keeps the stale type
# expect: RY001
x <- NULL
while (x) break
writer <- function() x <<- TRUE

## case: a closure nested two deep sees a later top-level writer
# no-diag
# `outer` starts with `state` unknown and the closure inherits it.
state <- NULL
outer <- function() {
  read <- function() if (state) 1
  read
}
setup <- function() state <<- TRUE

## case: a function's own `<<-` skips its local, so its closure keeps the stale type
# expect: RY001
# The write rebinds the file-level `x`; `read` still sees the local NULL.
x <- FALSE
outer <- function() {
  x <- NULL
  read <- function() if (x) 1
  x <<- TRUE
  read
}

## case: a `<<-` in a function's `local()` block reaches its earlier closure
# no-diag
# The block runs in a child of `f`'s frame, so the write rebinds `f`'s `x`.
f <- function() {
  x <- NULL
  g <- function() if (x) 1
  local({ x <<- TRUE })
  g
}

## case: a `<<-` passed bare to `local()` reaches the function's earlier closure
# no-diag
f <- function() {
  x <- NULL
  g <- function() if (x) 1
  local(x <<- TRUE)
  g
}

## case: a function's own `<<-` to an outer binding reaches its closure
# no-diag
# `h` binds no `x`, so the write rebinds the file-level `x` that `g` reads.
x <- NULL
h <- function() {
  g <- function() if (x) 1
  x <<- TRUE
  g
}

## case: a function's own complex `<<-` skips its local root
# expect: RY001
# R fetches the root of `a$b <<- v` from the enclosing frames, so the
# file-level `a` changes and `g` still reads the local `a$b` NULL.
a <- list(b = NULL)
f <- function() {
  a <- list(b = NULL)
  g <- function() if (a$b) 1
  a$b <<- TRUE
  g
}
