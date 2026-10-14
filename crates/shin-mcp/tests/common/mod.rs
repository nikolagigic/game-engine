//! Shared harness for the stdio integration tests: spawns the `shin-mcp`
//! binary and speaks MCP to it with hand-written JSON-RPC lines.

#![allow(dead_code)] // Each test binary uses a different subset.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// How long to wait for any one response (the first includes GPU setup).
const TIMEOUT: Duration = Duration::from_secs(60);

/// A running server with line-oriented access to its stdin and stdout.
pub struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    /// Every stdout line seen so far.
    seen: Vec<String>,
    /// The id of the next [`Self::call`] request.
    next_id: u64,
}

impl Server {
    /// Spawns `shin-mcp` with `args` and `RUST_LOG=debug`, stderr discarded.
    pub fn spawn(args: &[&str]) -> Self {
        Self::spawn_program(Path::new(env!("CARGO_BIN_EXE_shin-mcp")), args)
    }

    /// Spawns `program` (any MCP server on stdio, e.g. a generated game
    /// with `--mcp`) with `args` and `RUST_LOG=debug`, stderr discarded.
    pub fn spawn_program(program: &Path, args: &[&str]) -> Self {
        let mut child = Command::new(program)
            .args(args)
            .env("RUST_LOG", "debug")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|err| panic!("failed to spawn {}: {err}", program.display()));
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            seen: Vec::new(),
            next_id: 1000,
        }
    }

    /// Sends one JSON-RPC message.
    pub fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and returns the response with the same id.
    pub fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(TIMEOUT)
                .unwrap_or_else(|err| panic!("no response to `{method}` (id {id}): {err}"));
            self.seen.push(line.clone());
            let message = parse_json_line(&line);
            if message["id"] == json!(id) {
                return message;
            }
        }
    }

    /// The `initialize` request and `initialized` notification. Returns the
    /// `initialize` response.
    pub fn handshake(&mut self, client: &str) -> Value {
        let init = self.request(
            1,
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": client, "version": "0" }
            }),
        );
        self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        init
    }

    /// Calls tool `name` with `arguments` and returns the `tools/call`
    /// result, panicking on a JSON-RPC error.
    pub fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.next_id += 1;
        let response = self.request(
            self.next_id,
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert!(response.get("error").is_none(), "{name}: {response}");
        response["result"].clone()
    }

    /// [`Self::call`] for a tool that must succeed and return JSON text.
    pub fn call_json(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.call(name, arguments);
        assert_ne!(result["isError"], json!(true), "{name} failed: {result}");
        serde_json::from_str(first_text(&result))
            .unwrap_or_else(|err| panic!("{name} did not return JSON ({err}): {result}"))
    }

    /// Closes stdin, waits for the process to exit cleanly, and returns
    /// every stdout line it wrote.
    pub fn shutdown(mut self) -> Vec<String> {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "shin-mcp exited with {status}");
        self.seen.extend(self.lines.try_iter());
        self.seen
    }
}

/// `line` parsed as JSON, panicking with the line if it is not.
pub fn parse_json_line(line: &str) -> Value {
    serde_json::from_str(line)
        .unwrap_or_else(|err| panic!("stdout line is not JSON ({err}): {line}"))
}

/// The text of the first content block of a `tools/call` result (or of a
/// response holding one).
pub fn first_text(result: &Value) -> &str {
    let result = result.get("result").unwrap_or(result);
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content in {result}"))
}
