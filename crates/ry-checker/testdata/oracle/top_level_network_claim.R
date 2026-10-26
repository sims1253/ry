# oracle: must-warn RY124
# oracle-claim: RY124
# R's connection constructors differ in when they touch the network:
# `url()` defaults to `open = ""` and does not connect until opened,
# while `socketConnection()` defaults to `open = "a+"` and connects
# during the call itself (verified with Rscript: the open = ""
# socket below is created closed and silently, the defaulted one
# warns that 127.0.0.1:1 cannot be opened). RY124 therefore reports
# the defaulted `socketConnection` at file top level -- code a
# package runs while loading -- and stays silent for the two
# not-yet-open connections.
stopifnot(identical(formals(url)$open, ""))
stopifnot(identical(formals(socketConnection)$open, "a+"))

pending_url <- url("https://example.invalid/x")
stopifnot(!isOpen(pending_url))
close(pending_url)

pending_sock <- socketConnection("127.0.0.1", port = 1L, open = "")
stopifnot(!isOpen(pending_sock))
close(pending_sock)

# No `open` argument: the connection attempt happens during this
# top-level call. Nothing listens on port 1, so the attempt surfaces
# as a warning or an error; either proves the call reached the
# network stack instead of deferring.
attempted <- FALSE
opened <- withCallingHandlers(
  tryCatch(
    socketConnection("127.0.0.1", port = 1L, timeout = 1L),
    error = function(e) {
      attempted <<- TRUE
      NULL
    }
  ),
  warning = function(w) {
    attempted <<- TRUE
    invokeRestart("muffleWarning")
  }
)
stopifnot(attempted)
if (!is.null(opened)) {
  close(opened)
}
