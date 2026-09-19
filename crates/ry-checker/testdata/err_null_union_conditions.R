# expect: RY001, RY001
# Issue #362: the NULL component of a find-or-NULL helper's union return
# must reach the condition analysis at the call site. `switch(where, ...)`
# with where in `character | NULL` errors in R with "EXPR must be a
# length 1 vector", and `where == "path"` joins to `logical<0> |
# logical<1>`, the same deterministic "argument is of length zero" crash
# the local-NULL flow already reports.
locate_input <- function(input) {
  if (is.null(input)) {
    return(NULL)
  }
  "path"
}

where <- locate_input(NULL)
switch(where, path = 1L, 2L)
if (where == "path") 1L else 2L
