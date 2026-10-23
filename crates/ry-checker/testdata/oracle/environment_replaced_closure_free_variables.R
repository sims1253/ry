# oracle: must-pass
# R-side ground truth for `environment<-` (issue #716): once a closure's
# environment is replaced, its free variables resolve in the new
# environment, not in the frame that wrote the literal. The defining
# frame's `mkTheta <- NULL` placeholder (lme4's R CMD check idiom) is not
# what the body sees.
make_devfun <- function(rho) {
  mkTheta <- NULL
  devfun <- function(par) mkTheta(par)
  environment(devfun) <- rho
  devfun
}
rho <- new.env()
rho$mkTheta <- function(p) p * 2
stopifnot(identical(make_devfun(rho)(3), 6))

# lme4's mkdevfun picks the literal through an if/else value.
make_branch <- function(rho, linear) {
  mkTheta <- NULL
  ff <- if (linear) {
    function(par) mkTheta(par) + 1
  } else {
    function(pars) mkTheta(pars) - 1
  }
  environment(ff) <- rho
  ff
}
stopifnot(identical(make_branch(rho, TRUE)(3), 7))
stopifnot(identical(make_branch(rho, FALSE)(3), 5))
