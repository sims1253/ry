# no-diag
length <- function(x) 1L
masked <- function(x = x) length(x)
unused <- function(x) NULL
ordinary <- function(x = x) unused(x)
quoted <- function(x = x) base::quote(x)
conditional <- function(flag, x = x) base::typeof(if (flag) x else 1L)
short <- function(x = x) base::is.null(FALSE && x)
unrelated <- function(x = x) stats::typeof(x)
halted <- function(x = x) base::typeof({ base::stop("done"); x })
halted_identity <- function(x = x) base::identity({ result <- base::stop("done"); x })
nested_halt <- function(x = x) base::typeof({ base::identity(base::stop("done")); x })
block_halt <- function(x = x) base::typeof({ base::force({ base::stop("done") }); x })
returned <- function(x = x) base::typeof({ return(1L); x })
early_halt <- function(x = local) { base::stop("done"); base::typeof(x); local <- 1L }
default_halt <- function(x = { base::stop("done"); base::typeof(x) }) x
default_return <- function(x = { return(1L); base::typeof(x) }) x
default_replace <- function(x = { x <- 1L; base::typeof(x) }) x
default_branch <- function(flag, x = { if (flag) { base::stop("done"); base::typeof(x) } else 1L }) x
default_conditional_replace <- function(flag, x = { if (flag) x <- 1L else x <- 2L; base::typeof(x) }) x
default_binary_halt <- function(x = base::stop("done") + base::typeof(x)) x
default_index_halt <- function(x = base::stop("done")[base::typeof(x)]) x
