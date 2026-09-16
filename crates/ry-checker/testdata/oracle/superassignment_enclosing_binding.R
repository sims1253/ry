# oracle: must-pass
# `<<-` from a closure updates the enclosing binding, so the
# initialized-to-NULL json-parser pattern runs its loop: the writes
# execute before the condition evaluates. ry models every `<<-` target
# as unknown-typed after the definition (issue #374) and stays silent.
token <- NULL
read <- function(value) token <<- value
read(TRUE)
count <- 0L
while (token) {
  count <- count + 1L
  token <<- FALSE
}
stopifnot(identical(count, 1L))

state <- list(done = FALSE)
finish <- function() state$done <<- TRUE
finish()
stopifnot(isTRUE(state$done))

done <- NULL
outer <- function() {
  inner <- function() done <<- TRUE
  inner
}
outer()()
stopifnot(isTRUE(done))
