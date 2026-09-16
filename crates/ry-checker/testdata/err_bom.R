# expect: RY000
# A leading UTF-8 BOM is valid UTF-8, but R's parser rejects the file
# with "unexpected input" at 1:1 (only parse(keep.source = TRUE) accepts);
# ry flags it as an encoding RY000 instead of checking clean (#474).
# The bytes above are the real EF BB BF. The mid-string BOM in
# ok_bom_elsewhere.R is the adjacent idiom that stays quiet.
x <- 1
y <- x + 1
