# oracle: must-pass
# tryCatch catches an error; the result is the fallback. R succeeds.
v <- tryCatch(stop("boom"), error = function(e) 42)
print(v)
