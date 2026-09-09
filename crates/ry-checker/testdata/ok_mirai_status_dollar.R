# no-diag
# mirai::status() returns a named list ($connections, $daemons, plus
# $mirai/$memory under a dispatcher), so `$` access must stay valid
# (ry #382; r-typeshed mirai 0.0.2, merged a682775d). Covers positional
# and named `.compute` supply, miraiCluster and character profile
# arguments, and member extraction through the qualified call.
s <- mirai::status()
s$connections
s$daemons
s$mirai
s$memory
u <- mirai::status("profile")$mirai
v <- mirai::status(.compute = "profile")$connections
