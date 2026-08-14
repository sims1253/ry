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

    /// Initialize the server with a custom client-capabilities object, then
    /// send `initialized`. Used by tests that must advertise a capability
    /// the default [`initialize`](Self::initialize) does not (e.g.
    /// `workspace.configuration` pull).
    pub async fn initialize_with_capabilities(
        &mut self,
        root: &Path,
        capabilities: Value,
    ) -> io::Result<Value> {
        let root_uri = file_uri(root)?;
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": root_uri,
                    "capabilities": capabilities,
                    "workspaceFolders": [{"uri": root_uri, "name": "fixture"}]
                }),
            )
            .await?;
        self.notify("initialized", json!({})).await?;
        Ok(result)
    }

    /// Await a server-initiated request (one carrying both `id` and
    /// `method`) matching `method`, then reply with `result`.
    ///
    /// Server-initiated requests such as `workspace/configuration` arrive
    /// while the client is driving the session — e.g. during `initialized`
    /// or after a `workspace/didChangeConfiguration` notification on a pull
    /// client. This lets a test answer them with a scripted response so the
    /// server unblocks and proceeds (without it, a pull-config server stalls
    /// in the handler awaiting the reply).
    pub async fn respond_to_request(&mut self, method: &str, result: Value) -> io::Result<()> {
        let request = self
            .receive_matching(0, |message| {
                message.get("method").and_then(Value::as_str) == Some(method)
                    && message.get("id").is_some()
            })
            .await?;
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        self.client
            .send(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }))
            .await
    }

    pub async fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.client.request(method, params).await?;
        self.response(method, id).await
    }

    async fn response(&mut self, method: &str, id: u64) -> io::Result<Value> {
        let response = self
            .receive_matching(0, |message| {
                message.get("id") == Some(&json!(id)) && message.get("method").is_none()
            })
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
    /// Mark the currently routed transcript. Pair with
    /// `published_diagnostics_after` to require a publication caused by a
    /// later action rather than reusing an already-routed one.
    ///
    /// This counts read order, not server-side arrival order. A message
    /// already written by the server but not yet read receives a sequence
    /// after this mark. Callers that need a hard barrier should issue a
    /// request/response round-trip between the mark and the action, or
    /// ensure prior publications have already been consumed.
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

    /// P36-W8: wait for the target URI's `publishDiagnostics` after `mark`,
    /// then drain every other `publishDiagnostics` that arrives within
    /// `idle_timeout` of the previous message. Returns a `BTreeMap` of
    /// URI → diagnostic array. The initial wait is the quiescence signal
    /// (the notification arrives after the server's debounce); the drain
    /// captures multi-URI broadcasts. No sleep: the idle timeout detects
    /// end-of-stream, it does not wait for computation.
    pub async fn quiesce_diagnostics(
        &mut self,
        target_uri: &str,
        mark: u64,
        idle_timeout: std::time::Duration,
    ) -> io::Result<std::collections::BTreeMap<String, Vec<Value>>> {
        let target = self.published_diagnostics_after(target_uri, mark).await?;
        let mut result: std::collections::BTreeMap<String, Vec<Value>> =
            std::collections::BTreeMap::new();
        let collect = |msg: &Value, result: &mut std::collections::BTreeMap<String, Vec<Value>>| {
            if msg.get("method") == Some(&json!("textDocument/publishDiagnostics"))
                && let Some(uri) = msg.pointer("/params/uri").and_then(Value::as_str)
            {
                let diags = msg
                    .pointer("/params/diagnostics")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                result.insert(uri.to_string(), diags);
            }
        };
        collect(&target, &mut result);

        // Drain pending publications that arrived AFTER the mark (arrived
        // while waiting for the target). Older publications from prior
        // steps are left in the queue and ignored.
        let min_seq = mark.saturating_add(1);
        let pending_diags: Vec<Value> = self
            .pending
            .iter()
            .filter(|m| {
                m.sequence >= min_seq
                    && m.value.get("method") == Some(&json!("textDocument/publishDiagnostics"))
            })
            .map(|m| m.value.clone())
            .collect();
        for msg in &pending_diags {
            collect(msg, &mut result);
        }
        self.pending.retain(|m| {
            !(m.sequence >= min_seq
                && m.value.get("method") == Some(&json!("textDocument/publishDiagnostics")))
        });

        // Drain the stream with an idle timeout: keep reading while messages
        // arrive within `idle_timeout` of each other; stop on the first gap.
        let hard_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut last_received = tokio::time::Instant::now();
        loop {
            let next_deadline = (last_received + idle_timeout).min(hard_deadline);
            match tokio::time::timeout_at(next_deadline, self.client.receive()).await {
                Ok(Ok(value)) => {
                    self.sequence = self.sequence.wrapping_add(1);
                    last_received = tokio::time::Instant::now();
                    if value.get("method") == Some(&json!("textDocument/publishDiagnostics")) {
                        collect(&value, &mut result);
                    } else {
                        self.pending.push_back(RoutedMessage {
                            sequence: self.sequence,
                            value,
                        });
                    }
                }
                Ok(Err(e)) => {
                    return Err(io::Error::other(format!(
                        "receive error during quiesce drain: {e}"
                    )));
                }
                Err(_) => break,
            }
        }
        Ok(result)
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
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let value = tokio::time::timeout_at(deadline, self.client.receive())
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
