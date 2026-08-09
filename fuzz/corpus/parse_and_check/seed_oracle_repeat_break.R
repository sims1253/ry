# oracle: must-pass
# repeat loop with break; R succeeds.
i <- 0
repeat {
  i <- i + 1
  if (i >= 3) break
}
print(i)
