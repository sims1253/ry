# oracle: must-pass
# Neither `base::identity` nor the nested `inner` selects a project
# definition, so the forward cannot supply evidence for `inner`'s default.
outer <- function() {
  inner <- function(x = 0L) x + 1L
  inner(1L)
}
wrapper <- function(value = "text") base::identity(value)
stopifnot(identical(outer(), 2L))
stopifnot(identical(wrapper(), "text"))
