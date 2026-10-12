# no-diag
# An opaque attachment does not borrow base's paste0 signature.
box::use(./modules/assigned_paste0[paste0])
paste0("x") + 1L
box::use(mod/impl[paste0])
paste0("x") + 1L
