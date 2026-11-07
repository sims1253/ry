# oracle: must-pass
# `environment<-` on a value that is not a function only sets its
# ".Environment" attribute, so `ry dump-facts` keeps the type (#879),
# checked against GNU R by the ry-cli facts oracle.
# oracle-facts: placed
placed <- c(TRUE, FALSE)
environment(placed) <- globalenv()
