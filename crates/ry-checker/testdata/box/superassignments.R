# expect: RY118, RY118, RY118, RY118, RY118
# Superassignment writes through the module, creating no own export.
box::use(./modules/superassign_parent[b])
box::use(./modules/superassign_left[foo])
box::use(./modules/superassign_right[foo])
box::use(./modules/superassign_parenthesized[foo])
box::use(./modules/superassign_chained[foo])
