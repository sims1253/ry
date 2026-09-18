# no-diag
# A shadowed `all` inside the helper body disqualifies it, like the
# inline rule's shadowed-`all` silence (issue #479). The shadowing
# definition is file-global for the helper registry, so it lives in
# its own fixture: any helper parsing `all()` in this file resolves
# away from base.
all <- function(x) TRUE
shadowed_all_helper <- function(x) is.numeric(x) || all(is.na(x))
shadowed_all_caller <- function(v) {
  if (!shadowed_all_helper(v)) stop("bad")
  sqrt(v)
}
