# oracle: must-pass
# mirai::daemons + mirai: R succeeds (mirai attached).
library(mirai)
daemons(2)
m <- mirai(sqrt(2))
print(m)
daemons(0)
