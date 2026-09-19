# oracle: must-flag-only RY000
# Byte-for-byte copy of lintr's inst/example/bad.R at 990e578 (the
# posit corpus pin), prepended with these marker lines only. R's parser
# fails on the unterminated `{` block (Rscript: "unexpected end of
# input"), so whole-file suppression (#467) must leave RY000 as the
# file's only diagnostic (#470).
fun = function(one)
{
  one.plus.one <- oen + 1
  four <- newVar <- matrix(1:10,nrow = 2)
  four[ 1, ]
  txt <- 'hi'
  three <- two+ 1
  if(txt == 'hi') 4
  5}  
{
