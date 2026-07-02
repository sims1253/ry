# expect: RY040
# Named function bodies are currently NOT walked for diagnostics (PLAN.md
# finding 1). This fixture pins the desired behavior: arithmetic between a
# character and an integer inside a named function body must fire RY040.
# EXPECTED TO FAIL until Phase 1.3 / Phase 2.
f <- function() { x <- "hello" + 1 }
