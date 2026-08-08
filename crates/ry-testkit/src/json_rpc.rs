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
        let writer = child.stdin.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "server child stdin is not piped",
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "server child stdout is not piped",
            )
        })?;
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
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
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
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "server stdout closed",
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
}
