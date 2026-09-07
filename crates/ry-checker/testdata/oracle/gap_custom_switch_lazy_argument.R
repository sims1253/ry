# oracle: known-gap custom switch closures use the ordinary eager argument analysis
switch <- function(...) 1L
switch('ignored' + 1L)
