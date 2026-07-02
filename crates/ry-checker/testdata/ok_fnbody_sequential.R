# no-diag
# Sequential bindings inside an `if` branch must resolve each other:
# `out <- tmp + 1` must NOT fire RY010 on `tmp`. Today named function
# bodies are not walked at all (PLAN finding 1), so this fixture passes
# vacuously (zero diagnostics). It becomes meaningful once Phase 1.3 walks
# function bodies, at which point the per-statement scope-clone bug in the
# `Stmt::If` arm (PLAN Phase 1.3) would otherwise cause a spurious RY010
# on `out`.
g <- function(flag) {
  if (flag) {
    tmp <- 1
    out <- tmp + 1
  }
  NULL
}
