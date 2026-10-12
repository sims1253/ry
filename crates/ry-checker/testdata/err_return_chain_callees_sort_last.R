# expect: RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040, RY040
# A character return travels up a 12-deep call chain whatever the
# function names. Each callee sorts after its caller (f01 -> f02 -> ... -> f12).
f01 <- function() f02()
f02 <- function() f03()
f03 <- function() f04()
f04 <- function() f05()
f05 <- function() f06()
f06 <- function() f07()
f07 <- function() f08()
f08 <- function() f09()
f09 <- function() f10()
f10 <- function() f11()
f11 <- function() f12()
f12 <- function() "x"
r01 <- function() f01() + 1
r02 <- function() f02() + 1
r03 <- function() f03() + 1
r04 <- function() f04() + 1
r05 <- function() f05() + 1
r06 <- function() f06() + 1
r07 <- function() f07() + 1
r08 <- function() f08() + 1
r09 <- function() f09() + 1
r10 <- function() f10() + 1
r11 <- function() f11() + 1
r12 <- function() f12() + 1
