# expect-parse-error
# Broken syntax. tree-sitter produces a recovered tree with ERROR/MISSING
# nodes; today `root.has_error()` is never consulted (PLAN.md finding 3 /
# Phase 1.2) so no RY000 is emitted. EXPECTED TO FAIL until Phase 1.2
# introduces syntax-error reporting.
f <- function( {
  broken syntax ((
