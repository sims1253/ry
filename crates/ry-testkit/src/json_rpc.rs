use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// A framed JSON-RPC client owning a real stdio server process.
pub struct JsonRpcProcess {
    child: Child,
    reader: BufReader<ChildStdout>,
    writer: ChildStdin,
    next_id: u64,
}

impl JsonRpcProcess {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        Self::from_child(child)
    }

    /// Attach to a spawned child whose stdin and stdout were piped.
    pub fn from_child(mut child: Child) -> io::Result<Self> {
        let Some(writer) = child.stdin.take() else {
            kill_and_wait(&mut child);
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "server child stdin is not piped",
            ));
        };
        let Some(stdout) = child.stdout.take() else {
            kill_and_wait(&mut child);
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "server child stdout is not piped",
            ));
        };
        Ok(Self {
            child,
            reader: BufReader::new(stdout),
            writer,
            next_id: 1,
        })
    }

    pub fn request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    pub fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    pub fn send(&mut self, message: &Value) -> io::Result<()> {
        let frame = encode(message)?;
        self.writer.write_all(&frame)?;
        self.writer.flush()
    }

    pub fn receive(&mut self) -> io::Result<Value> {
        decode_blocking(&mut self.reader)
    }

    pub fn receive_until(
        &mut self,
        mut predicate: impl FnMut(&Value) -> bool,
        message_limit: usize,
    ) -> io::Result<Value> {
        for _ in 0..message_limit {
            let message = self.receive()?;
            if predicate(&message) {
                return Ok(message);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no matching JSON-RPC message in {message_limit} messages"),
        ))
    }

    pub fn child_id(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for JsonRpcProcess {
    fn drop(&mut self) {
        kill_and_wait(&mut self.child);
    }
}

fn kill_and_wait(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// A framed JSON-RPC client over arbitrary async streams, used with
/// `ry_lsp::run_with` and Tokio duplex streams.
pub struct AsyncJsonRpcClient<R, W> {
    reader: tokio::io::BufReader<R>,
    writer: W,
    next_id: u64,
}

impl<R, W> AsyncJsonRpcClient<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader: tokio::io::BufReader::new(reader),
            writer,
            next_id: 1,
        }
    }

    pub async fn request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await?;
        Ok(id)
    }

    pub async fn request_without_params(&mut self, method: &str) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        }))
        .await?;
        Ok(id)
    }

    pub async fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
        .await
    }

    pub async fn send(&mut self, message: &Value) -> io::Result<()> {
        let frame = encode(message)?;
        self.writer.write_all(&frame).await?;
        self.writer.flush().await
    }

    pub async fn receive(&mut self) -> io::Result<Value> {
        decode_async(&mut self.reader).await
    }

    pub async fn receive_until(
        &mut self,
        mut predicate: impl FnMut(&Value) -> bool,
        message_limit: usize,
    ) -> io::Result<Value> {
        for _ in 0..message_limit {
            let message = self.receive().await?;
            if predicate(&message) {
                return Ok(message);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no matching JSON-RPC message in {message_limit} messages"),
        ))
    }
}

fn encode(message: &Value) -> io::Result<Vec<u8>> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn decode_blocking(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut content_length = None;
    let mut header_bytes = 0;
    loop {
        let mut header = String::new();
        let remaining = MAX_MESSAGE_BYTES - header_bytes;
        let mut limited = std::io::Read::take(&mut *reader, (remaining + 1) as u64);
        let bytes_read = limited.read_line(&mut header)?;
        if bytes_read > remaining {
            return Err(headers_too_large());
        }
        if bytes_read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server stdout closed",
            ));
        }
        header_bytes += bytes_read;
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(length) = parse_content_length(&header)? {
            content_length = Some(length);
        }
    }
    let length = checked_length(content_length)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

async fn decode_async<R>(reader: &mut tokio::io::BufReader<R>) -> io::Result<Value>
where
    R: AsyncRead + Unpin,
{
    let mut content_length = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).await? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server output closed",
            ));
        }
        if header == "\r\n" || header == "\n" {
            break;
        }
        if let Some(length) = parse_content_length(&header)? {
            content_length = Some(length);
        }
    }
    let length = checked_length(content_length)?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn headers_too_large() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("JSON-RPC headers exceed {MAX_MESSAGE_BYTES} bytes"),
    )
}

fn parse_content_length(header: &str) -> io::Result<Option<usize>> {
    let Some((name, value)) = header.trim_end().split_once(':') else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "malformed JSON-RPC header",
        ));
    };
    if !name.eq_ignore_ascii_case("content-length") {
        return Ok(None);
    }
    value
        .trim()
        .parse()
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn checked_length(content_length: Option<usize>) -> io::Result<usize> {
    let length = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length header")
    })?;
    if length > MAX_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("JSON-RPC message is too large: {length} bytes"),
        ));
    }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn from_child_reaps_child_when_stdin_is_missing() {
        let child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();

        let error = match JsonRpcProcess::from_child(child) {
            Ok(_) => panic!("child without piped stdin was accepted"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "invalid child {pid} was not reaped"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn from_child_reaps_child_when_stdout_is_missing() {
        let child = Command::new("sh")
            .args(["-c", "sleep 30"])
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();

        let error = match JsonRpcProcess::from_child(child) {
            Ok(_) => panic!("child without piped stdout was accepted"),
            Err(error) => error,
        };

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "invalid child {pid} was not reaped"
        );
    }

    #[test]
    fn blocking_decoder_accepts_additional_headers() {
        let bytes = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: 7\r\n\r\n{\"x\":1}";
        let mut reader = BufReader::new(&bytes[..]);
        assert_eq!(decode_blocking(&mut reader).unwrap(), json!({"x": 1}));
    }

    #[test]
    fn decoder_requires_framing_instead_of_accepting_stdout_noise() {
        let bytes = b"log output\nContent-Length: 2\r\n\r\n{}";
        let mut reader = BufReader::new(&bytes[..]);
        assert!(decode_blocking(&mut reader).is_err());
    }

    #[test]
    fn blocking_decoder_rejects_oversized_headers() {
        let bytes = vec![b'x'; MAX_MESSAGE_BYTES + 1];
        let mut reader = BufReader::new(bytes.as_slice());

        let error = decode_blocking(&mut reader).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            error.to_string(),
            format!("JSON-RPC headers exceed {MAX_MESSAGE_BYTES} bytes")
        );
    }

    // ── cross-mode subprocess framing ──────────────────────────

    /// encode/decode round-trip preserves the message exactly.
    ///
    /// Protects the framing seam: if `encode` wrote a wrong Content-Length
    /// or a malformed header terminator, the decoder would produce a
    /// different message or fail. This catches a framing mismatch between
    /// what the testkit client sends and what it expects to receive.
    #[test]
    fn encode_decode_round_trip_preserves_message() {
        let messages = [
            json!({"jsonrpc": "2.0", "id": 1, "result": {"capabilities": {}}}),
            json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "file:///x.R", "diagnostics": []}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null}),
        ];
        for message in &messages {
            let frame = encode(message).expect("encode");
            let mut reader = BufReader::new(frame.as_slice());
            let decoded = decode_blocking(&mut reader).expect("decode");
            assert_eq!(&decoded, message);
        }
    }

    /// the Content-Length header matches the body byte length
    /// exactly, and the header/body separator is the correct CRLFCRLF.
    /// A wrong separator or miscounted length silently truncates or
    /// merges messages across the subprocess boundary.
    #[test]
    fn encoded_frame_has_exact_content_length_and_separator() {
        let message = json!({"jsonrpc": "2.0", "id": 42, "result": "ok"});
        let frame = encode(&message).expect("encode");

        // Search for the CRLFCRLF separator at the byte level to avoid
        // string-escape ambiguity in the test source.
        let separator = b"\r\n\r\n";
        let header_end = frame
            .windows(4)
            .position(|w| w == separator)
            .expect("header separator");
        let header = std::str::from_utf8(&frame[..header_end]).expect("header is ASCII");
        let header = header.trim_end_matches(['\r', '\n']);
        let body = &frame[header_end + separator.len()..];

        assert!(
            header.starts_with("Content-Length: "),
            "frame must start with Content-Length header, got: {header:?}",
        );
        let declared: usize = header["Content-Length: ".len()..]
            .parse()
            .expect("Content-Length is an integer");
        assert_eq!(
            declared,
            body.len(),
            "Content-Length {declared} does not match actual body {} bytes",
            body.len(),
        );

        let decoded: Value = serde_json::from_slice(body).expect("body is valid JSON");
        assert_eq!(decoded, message);
    }

    /// multiple framed messages in one buffer decode in sequence.
    ///
    /// The subprocess can write several JSON-RPC messages back-to-back on
    /// stdout (e.g., a log-message notification followed by a response).
    /// The decoder must handle them one at a time without merging or
    /// skipping.
    #[test]
    fn decoder_handles_multiple_back_to_back_messages() {
        let first =
            json!({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"message": "hi"}});
        let second = json!({"jsonrpc": "2.0", "id": 1, "result": {"serverInfo": {"name": "ry"}}});

        let mut buffer = encode(&first).expect("encode first");
        buffer.extend(encode(&second).expect("encode second"));

        let mut reader = BufReader::new(buffer.as_slice());
        let decoded_first = decode_blocking(&mut reader).expect("decode first");
        let decoded_second = decode_blocking(&mut reader).expect("decode second");
        // After two messages, the buffer should be fully consumed.
        let trailing = decode_blocking(&mut reader);
        assert!(trailing.is_err(), "unexpected third message: {trailing:?}");

        assert_eq!(decoded_first, first);
        assert_eq!(decoded_second, second);
    }
}
