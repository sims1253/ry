# expect: RY109, RY109, RY109, RY109, RY109, RY109, RY109, RY109
# The `quoted <- function(x = x) base::quote(x)` line defuses its promise
# through a reviewed capture helper and stays quiet.
# RY098 must stay silent on every line (identity/quote skip or mask the
# force); RY109 now flags each self-referential default anyway.
quoted <- function(x = x) base::quote(x)
identity <- function(x) NULL
masked <- function(x = x) identity(x)
short <- function(x = x) base::identity(FALSE && x)
branch <- function(x = x) base::identity(if (FALSE) x else 1L)
conditional <- function(flag, x = x) base::identity(if (flag) x else 1L)
force <- function(x) NULL
force_masked <- function(x = x) force(x)
force_short <- function(x = x) base::force(FALSE && x)
force_branch <- function(x = x) base::force(if (FALSE) x else 1L)
force_conditional <- function(flag, x = x) base::force(if (flag) x else 1L)
