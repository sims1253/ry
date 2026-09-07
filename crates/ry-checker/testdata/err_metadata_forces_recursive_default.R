# expect: RY098, RY098, RY098, RY098, RY098, RY098
by_type <- function(x = x) base::typeof(x)
by_length <- function(x = x) base::length(x)
by_null <- function(x = x) base::is.null(x)
by_function <- function(x = x) base::is.function(x)
by_invisible <- function(x = x) base::invisible(x = x)
early <- function(x = local) { base::typeof(x); local <- 1L }
