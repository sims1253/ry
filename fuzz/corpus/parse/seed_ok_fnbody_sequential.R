# no-diag
# Sequential bindings inside an `if` branch must resolve each other:
# `out <- tmp + 1` must NOT fire RY010 on `tmp`. Guards against a
# per-statement scope-clone bug in the `Stmt::If` arm that once caused
# a spurious RY010 on `out`.
g <- function(flag) {
  if (flag) {
    tmp <- 1
    out <- tmp + 1
  }
  NULL
}
