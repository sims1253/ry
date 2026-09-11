# expect: RY061, RY061, RY061
# True-positive controls around the mirai::status fix: `$` on provably
# atomic receivers must keep firing (double literal, integer-returning
# nchar(), and mirai::unresolved()'s logical(1) result).
x <- 1.5
a <- x$foo
n <- nchar("abc")
b <- n$len
m <- mirai::mirai(1 + 1)
i <- mirai::unresolved(m)
c <- i$extra
