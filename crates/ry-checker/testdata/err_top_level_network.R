# expect: RY124, RY124
season <- utils::download.file("https://example.invalid/season.csv", tempfile())
.onLoad <- function(libname, pkgname) {
  curl::curl_download("https://example.invalid/cache.csv", tempfile())
}
