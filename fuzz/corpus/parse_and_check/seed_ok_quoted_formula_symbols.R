# no-diag
# quote()/bquote()/substitute() capture language objects; bare names
# inside them are symbols, not variable reads in the current scope.
build_mmrm_call <- function(covariance = "us", visit_var = "AVISIT") {
  cov_sym <- as.symbol(covariance)
  visit_sym <- as.symbol(visit_var)
  cov_call <- bquote(.(cov_sym)(.(visit_sym) | USUBJID))
  fixed_formula <- as.formula("CHG ~ ARM + AVISIT")
  full_call <- bquote(
    mmrm::mmrm(
      call("=~", quote(CHG), bquote(.(fixed_formula)[[3]] + .(cov_call))),
      method = "Satterthwaite"
    )
  )
  lhs <- quote(CHG)
  list(full_call = full_call, lhs = lhs)
}
