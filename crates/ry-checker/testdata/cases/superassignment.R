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
