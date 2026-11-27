//! Single stdout seam for batch CLI commands (#920).
//!
//! Writes stdout through one helper that turns `io::ErrorKind::BrokenPipe`
//! into a quiet process exit, replacing the Unix-only `unsafe` SIGPIPE reset
//! while keeping real I/O errors visible.

use std::io::{self, Write};
use std::process::ExitCode;

use miette::{IntoDiagnostic, Result};

/// Write `text` to `writer`, returning `Ok(true)` on success and `Ok(false)`
/// when the downstream reader closed the pipe (`BrokenPipe`).
fn write_to<W: Write>(mut writer: W, text: &str) -> io::Result<bool> {
    match writer
        .write_all(text.as_bytes())
        .and_then(|()| writer.flush())
    {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error),
    }
}

/// Write raw text to stdout. Exits the process cleanly on `BrokenPipe`.
pub(crate) fn write_stdout(text: &str) -> Result<()> {
    let stdout = io::stdout();
    if !write_to(stdout.lock(), text).into_diagnostic()? {
        std::process::exit(0);
    }
    Ok(())
}

/// Write a line (text + `\n`) to stdout. Exits the process cleanly on `BrokenPipe`.
pub(crate) fn writeln_stdout(text: &str) -> Result<()> {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    write_stdout(&line)
}

/// Write raw bytes to stdout. Exits the process cleanly on `BrokenPipe`.
pub(crate) fn write_stdout_bytes(bytes: &[u8]) -> Result<()> {
    let stdout = io::stdout();
    let mut lock = stdout.lock();
    match lock.write_all(bytes).and_then(|()| lock.flush()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => std::process::exit(0),
        Err(error) => Err(error).into_diagnostic(),
    }
}

/// Write raw text to stderr without panicking on `BrokenPipe` (which exits
/// quietly like batch stdout).
pub(crate) fn write_stderr(text: &str) {
    let stderr = io::stderr();
    match write_to(stderr.lock(), text) {
        Ok(true) => {}
        Ok(false) => std::process::exit(0),
        Err(_) => {}
    }
}

/// Write a line (text + `\n`) to stderr without panicking on `BrokenPipe`.
pub(crate) fn writeln_stderr(text: &str) {
    let mut line = String::with_capacity(text.len() + 1);
    line.push_str(text);
    line.push('\n');
    write_stderr(&line);
}

#[macro_export]
macro_rules! eprintln_safe {
    () => {
        $crate::output::writeln_stderr("")
    };
    ($($arg:tt)*) => {
        $crate::output::writeln_stderr(&format!($($arg)*))
    };
}

#[macro_export]
macro_rules! eprint_safe {
    ($($arg:tt)*) => {
        $crate::output::write_stderr(&format!($($arg)*))
    };
}

/// Handle a `clap::Error` without panicking if stdout or stderr is closed.
/// Delegates to `err.print()` so clap's stream-aware color policy stays
/// intact; treats `BrokenPipe` as success only for help/version on stdout,
/// preserving clap's error exit code on invalid invocations.
pub(crate) fn handle_clap_error(err: clap::Error) -> ExitCode {
    let code = ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(1));
    let target_is_stdout = !err.use_stderr();
    match err.print() {
        Ok(()) => code,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {
            if target_is_stdout {
                ExitCode::SUCCESS
            } else {
                code
            }
        }
        Err(_) => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct OtherErrorWriter;

    impl Write for OtherErrorWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn broken_pipe_returns_false_without_error() {
        assert!(!write_to(BrokenPipeWriter, "hello\n").unwrap());
    }

    #[test]
    fn non_pipe_io_errors_still_propagate() {
        let error = write_to(OtherErrorWriter, "hello\n").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn normal_writes_succeed() {
        let mut buf = Vec::new();
        assert!(write_to(&mut buf, "hello\n").unwrap());
        assert_eq!(buf, b"hello\n");
    }
}
