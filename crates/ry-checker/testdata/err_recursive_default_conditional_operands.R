# expect: RY109, RY109, RY109, RY109, RY109
# The `quoted <- function(x = x) TRUE && base::quote(x)` line defuses its
# promise through a reviewed capture helper and stays quiet.
# Scalar short-circuit operands may never evaluate the default at runtime,
# but the default itself can never evaluate, so RY109 warns without a
# forcing proof.
skip_and <- function(x = x) FALSE && x
skip_or <- function(x = x) TRUE || x
dynamic_and <- function(flag, x = x) flag && x
dynamic_or <- function(flag, x = x) flag || x
nested_skip <- function(x = x) TRUE && (TRUE || x)
quoted <- function(x = x) TRUE && base::quote(x)
