# expect: RY040
# Named function bodies are currently NOT walked for diagnostics (PLAN.md
# finding 1). The arithmetic error sits inside an `if` inside the named
# function body; once bodies are walked this must fire RY040.
# EXPECTED TO FAIL until Phase 1.3 / Phase 2.
f <- function(flag) {
  if (flag) {
    x <- "hello" + 1
  }
}
