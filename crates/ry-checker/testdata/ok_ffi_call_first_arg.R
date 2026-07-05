# no-diag
# FFI primitives (.Call/.C/.Fortran/.External/.External2/.Internal) take
# a native entry-point symbol as the first argument, written as a bare
# identifier. It must NOT be treated as an unbound variable (RY010).
# Remaining args are inferred normally.
glue_c <- function(x) .Call(glue_, x)
trim_c <- function(x) .Fortran(trim_, x)
ext_c <- function(x, name) .External(do_thing, x, name)
