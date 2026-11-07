# oracle: must-pass
# `ry dump-facts` types after the base setters #879 audits, checked
# against GNU R by the ry-cli facts oracle. `comment<-` and `attr<-` with
# an ordinary name keep the whole value. `levels<-`, `names<-`, `dim<-`
# and `dimnames<-` keep a classless value's mode, length and class.
# `is.na<-` keeps a classless vector's mode and may grow it.
# (`environment<-` has its own fixture: it makes every S3 lookup in the
# file conservative.)
# oracle-facts: commented commented_frame tagged leveled renamed shaped labeled missing grown
commented <- list(a = 1)
comment(commented) <- "k"
commented_frame <- data.frame(a = 1:2)
comment(commented_frame) <- "c"
tagged <- c(1, 2)
attr(tagged, "units") <- "cm"
leveled <- 1:3
levels(leveled) <- c("a", "b")
renamed <- list(a = 1, b = "x")
names(renamed) <- c("p", "q")
shaped <- 1:4
dim(shaped) <- c(2L, 2L)
labeled <- 1:4
dim(labeled) <- c(2L, 2L)
dimnames(labeled) <- list(c("r1", "r2"), c("c1", "c2"))
missing <- "a"
is.na(missing) <- 1
grown <- c(1.5, 2)
is.na(grown) <- 5
