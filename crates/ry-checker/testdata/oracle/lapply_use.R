# oracle: must-pass
# lapply applies a function over a list; R succeeds.
# KNOWN LIMITATION: ry currently emits a spurious RY040 on the
# anonymous callback `function(x) x * 2` (it mis-infers the callback's
# arithmetic on the opaque parameter `x`). The oracle surfaces this
# false positive. Tracked as a higher-order-callback inference
# improvement.
out <- lapply(list(1, 2, 3), function(x) x * 2)
print(out)
