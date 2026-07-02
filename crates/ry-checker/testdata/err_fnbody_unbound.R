# expect: RY010
# Named function bodies are currently NOT walked for diagnostics (PLAN.md
# finding 1). This fixture pins the desired behavior: a reference to an
# undefined variable inside a named function body must fire RY010.
# EXPECTED TO FAIL until Phase 1.3 / Phase 2.
f <- function() { y <- undefined_variable_xyz }
