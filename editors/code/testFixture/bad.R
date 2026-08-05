# Test fixture for the ry language server.
#
# Triggers a known diagnostic (RY040, invalid arithmetic) that survives the
# RY010 ignore list in ry.toml. The unbound reference on the last line is
# RY010 and must be suppressed.
y <- "a" + 1L
x <- undefined_thing
