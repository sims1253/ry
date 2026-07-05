# oracle: must-pass
# R: a super-assignment inside a function to a name not present in any
# enclosing scope creates a binding in the global environment and succeeds.
# (Note: top-level `x <<- v` to a missing name never errors in R — it
# creates the binding — so this fixture exercises the assignment through a
# function call to keep the R side well-defined.) ry emits no
# diagnostics for super-assignment, matching R's success.
f <- function() { super_assign_target <<- 1 }
f()
print(super_assign_target)
