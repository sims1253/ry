# no-diag
# Argument matching rejects these calls before forcing the recursive default.
wrong_name <- function(x = x) base::identity(unused = x)
extra <- function(x = x) base::identity(1L, unused = x)
force_wrong_name <- function(x = x) base::force(unused = x)
force_extra <- function(x = x) base::force(1L, unused = x)
