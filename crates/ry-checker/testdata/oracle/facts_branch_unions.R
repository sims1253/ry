# oracle: must-pass
# `ry dump-facts` claims for values chosen by a branch ry cannot decide.
# R takes one branch; its typeof() must be one of the union's member modes
# and its length must meet the union's bound (#858).
# oracle-facts: flag either widths classed
flag <- runif(1) > 2
either <- if (flag) 1L else "a"
widths <- if (flag) 1:3 else c(1.5, 2)
classed <- if (flag) structure(1, class = "a") else structure(2, class = "a")
