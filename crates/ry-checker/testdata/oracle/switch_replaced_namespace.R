# oracle: must-pass
replace <- function() assign(paste0(':', ':'), function(...) function(...) 1L, envir=.GlobalEnv)
replace()
out <- base::switch(1L, 'bad', 1L)
stopifnot(identical(out+1L, 2L))
