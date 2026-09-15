# expect: RY109, RY109, RY109, RY109
# RY098 must stay silent on every line (argument matching rejects these
# calls before any force); RY109 now flags each self-referential default
# anyway.
# Argument matching rejects these calls before forcing the recursive default.
wrong_name <- function(x = x) base::identity(unused = x)
extra <- function(x = x) base::identity(1L, unused = x)
force_wrong_name <- function(x = x) base::force(unused = x)
force_extra <- function(x = x) base::force(1L, unused = x)
