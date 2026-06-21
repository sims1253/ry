# expect: RY050
# `print(x)` on a value whose class has no `print.<class>` method (and no
# user-defined default) emits RY050. `print.default` exists in base R, so
# we're confident `print` uses S3 dispatch and the missing specific method
# is worth flagging.
x <- structure(list(), class = "undefined")
print(x)
