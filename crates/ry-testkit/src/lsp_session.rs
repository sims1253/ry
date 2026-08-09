use crate::AsyncJsonRpcClient;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io;
use std::path::Path;
use tokio::io::{AsyncRead, AsyncWrite};

struct RoutedMessage {
    sequence: u64,
    value: Value,
}

/// Stateful, protocol-only LSP client for production-path integration tests.
///
/// Unlike `AsyncJsonRpcClient::receive_until`, this router retains messages
/// interleaved before the sought response/publication. Crate-specific tests
/// own the server launcher so this testkit never depends on a production LSP.
pub struct LspSession<R, W> {
    client: AsyncJsonRpcClient<R, W>,
    pending: VecDeque<RoutedMessage>,
    sequence: u64,
}

impl<R, W> LspSession<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            client: AsyncJsonRpcClient::new(reader, writer),
            pending: VecDeque::new(),
            sequence: 0,
        }
    }

    pub async fn initialize(&mut self, root: &Path) -> io::Result<Value> {
        let root_uri = file_uri(root)?;
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": {},
                    "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result)
    }

    pub async fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.client.request(method, params).await?;
        self.response(method, id).await
    }

    async fn response(&mut self, method: &str, id: u64) -> io::Result<Value> {
        let response = self
            .receive_matching(0, |message| message.get("id") == Some(&json!(id)))
            .await?;
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(format!("{method} failed: {error}")));
        }
        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    pub async fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.client.notify(method, params).await
    }

    pub async fn open(&mut self, uri: &str, version: i32, text: &str) -> io::Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "r", "version": version, "text": text
            }}),
        )
        .await
    }

    pub async fn change(&mut self, uri: &str, version: i32, changes: Value) -> io::Result<()> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": changes
            }),
        )
        .await
    }

    /// Mark the currently routed transcript. Pair with
    /// `published_diagnostics_after` to require a publication caused by a
    /// later action rather than reusing an already-routed one.
    pub fn publication_mark(&self) -> u64 {
        self.sequence
    }

    pub async fn published_diagnostics_after(&mut self, uri: &str, mark: u64) -> io::Result<Value> {
        self.receive_matching(mark.saturating_add(1), |message| {
            message.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && message.pointer("/params/uri") == Some(&json!(uri))
        })
        .await
    }

    /// Complete the protocol shutdown. The owning adapter remains
    /// responsible for dropping this session and bounded-joining its server.
    pub async fn shutdown(&mut self) -> io::Result<()> {
        let id = self.client.request_without_params("shutdown").await?;
        let _ = self.response("shutdown", id).await?;
        self.notify("exit", Value::Null).await
    }

    async fn receive_matching(
        &mut self,
        minimum_sequence: u64,
        predicate: impl Fn(&Value) -> bool,
    ) -> io::Result<Value> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|message| message.sequence >= minimum_sequence && predicate(&message.value))
        {
            return Ok(self.pending.remove(index).unwrap().value);
        }
        loop {
            let value =
                tokio::time::timeout(std::time::Duration::from_secs(5), self.client.receive())
                    .await
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::TimedOut, "JSON-RPC receive timed out")
                    })??;
            self.sequence = self.sequence.wrapping_add(1);
            let sequence = self.sequence;
            if sequence >= minimum_sequence && predicate(&value) {
                return Ok(value);
            }
            self.pending.push_back(RoutedMessage { sequence, value });
        }
    }
}

pub fn file_uri(path: &Path) -> io::Result<String> {
    let path = path.canonicalize()?;
    url::Url::from_file_path(&path)
        .map(String::from)
        .map_err(|()| io::Error::new(io::ErrorKind::InvalidInput, "path cannot form a file URI"))
}
