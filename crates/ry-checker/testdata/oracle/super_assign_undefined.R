# oracle: must-pass
# R: a super-assignment inside a function to a name not present in any
# enclosing scope creates a binding in the global environment and succeeds.
# (Note: top-level `x <<- v` to a missing name never errors in R — it
# creates the binding — so this fixture exercises the assignment through a
# function call to keep the R side well-defined.) The ry side currently
# drops the `<<-` statement (PLAN finding 6.1); today this fixture passes
# the oracle because ry emits no diagnostics, which is the correct neutral
# behavior for a Phase 0 seed.
f <- function() { super_assign_target <<- 1 }
f()
print(super_assign_target)
