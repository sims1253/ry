# oracle: must-flag-only RY000
# R cannot parse the incomplete `function(` header, so nothing in the
# file runs -- the same broken-region premise as syntax_error_claim.R.
# The unbound `later_use` inside the broken region would be RY010 on a
# clean file; under a recovered tree the semantic rule must stay silent
# so only RY000 reports (issue #380).
x <- function(
  later_use
