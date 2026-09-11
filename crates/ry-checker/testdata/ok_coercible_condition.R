# no-diag
# R coerces `if`/`while` conditions beyond logical scalars (#373): scalar
# raw values coerce numerically, and scalar character values coerce when
# their text is one of the eight accepted literals. The value of a
# computed scalar string (Sys.getenv flags) is unknowable statically, so
# those conditions stay silent rather than claim R rejects them.
if (Sys.getenv("FLAG")) {
  print(1)
}
while (Sys.getenv("LOOP")) {
  break
}
flag <- Sys.getenv("OTHER")
if (flag) {
  print(2)
}
a <- if ("TRUE") 1 else 2
b <- if ("T") 1 else 2
c <- if ("true") 1 else 2
d <- if ("False") 1 else 2
e <- if ("\x54RUE") 1 else 2
f <- if ("\106ALSE") 1 else 2
g <- if (r"(TRUE)") 1 else 2
h <- if (as.raw(1)) 1 else 2
i <- if ((("TRUE"))) 1 else 2
j <- if (complex(real = 1)) 1 else 2
k <- if (vector("complex", 1)) 1 else 2
l <- if (vector("raw", 1)) 1 else 2
m <- if (complex(real = 0, imaginary = 1)) 1 else 2
# NA_complex_ is complex<len=1>; R errors on it at runtime (value, not
# mode, decides). Silence is the uncertainty policy; NA policy is #354.
n <- if (NA_complex_) 1 else 2
