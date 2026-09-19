# no-diag
# The guard idioms that make find-or-NULL unions safe must keep RY001
# silent (issue #362): each guard removes the NULL member in the
# continuation, so the union refinement composes with the existing
# null-check handling instead of reporting through a guard.
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}

# Early-return guard: only the non-null remainder continues.
early_return <- function(input) {
  where <- locate_input(input)
  if (is.null(where)) {
    return(NULL)
  }
  switch(where, path = 1L, 2L)
  if (where == "path") 1L else 2L
}

# Replacement guard: the rebind and the remainder join without the NULL.
replacement <- function(input) {
  where <- locate_input(input)
  if (is.null(where)) {
    where <- "none"
  }
  switch(where, path = 1L, none = 2L)
  if (where == "path") 1L else 2L
}

# else-return mirror: the then arm carries the remainder.
else_return <- function(input) {
  where <- locate_input(input)
  if (!is.null(where)) {
    1L
  } else {
    return(NULL)
  }
  if (where == "path") 1L else 2L
}

# Short-circuit guard inside the condition.
short_circuit <- function(input) {
  where <- locate_input(input)
  if (!is.null(where) && where == "path") {
    1L
  } else {
    2L
  }
}

# if/else split: the else arm sees the remainder branch-locally.
split <- function(input) {
  where <- locate_input(input)
  if (is.null(where)) {
    NULL
  } else if (where == "path") {
    1L
  } else {
    2L
  }
}

# Non-condition uses of the union stay silent.
consume <- function(input) {
  where <- locate_input(input)
  print(where)
  paste0(where, "!")
  where[[1L]]
}
