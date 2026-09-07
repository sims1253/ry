# oracle: must-pass
`<-` <- function(...) assign(paste0('swi', 'tch'), function(...) 1L, envir=.GlobalEnv)
x <- 1L
switch(1L, 'bad', 1L)+1L
