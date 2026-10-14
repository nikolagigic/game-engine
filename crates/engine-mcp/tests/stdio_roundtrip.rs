//! Spawns the `engine-mcp` binary and speaks MCP to it over stdio with
//! hand-written JSON-RPC lines: handshake, `tools/list`, an unknown tool
//! (must be a JSON-RPC error, not a crash), then `engine.info`.
//!
//! Runs with `RUST_LOG=debug` and checks that every stdout line is JSON, so
//! no log output leaks into the protocol channel.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

/// How long to wait for any one response (the first includes GPU setup).
const TIMEOUT: Duration = Duration::from_secs(60);

/// A running server with line-oriented access to its stdin and stdout.
struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
    /// Every stdout line seen so far.
    seen: Vec<String>,
}

impl Server {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_engine-mcp"))
            .args(["--width", "64", "--height", "36"])
            .env("RUST_LOG", "debug")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn engine-mcp");
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
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a request and returns the response with the same id.
    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
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

    /// Closes stdin, waits for the process to exit cleanly, and returns
    /// every stdout line it wrote.
    fn shutdown(mut self) -> Vec<String> {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "engine-mcp exited with {status}");
        self.seen.extend(self.lines.try_iter());
        self.seen
    }
}

fn parse_json_line(line: &str) -> Value {
    serde_json::from_str(line)
        .unwrap_or_else(|err| panic!("stdout line is not JSON ({err}): {line}"))
}

/// The text of the first content block of a `tools/call` result.
fn first_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content in {response}"))
}

#[test]
fn stdio_roundtrip() {
    let mut server = Server::spawn();

    let init = server.request(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "stdio_roundtrip", "version": "0" }
        }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "engine-mcp", "{init}");
    server.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let list = server.request(2, "tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("no tools in {list}"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"engine.info"), "{names:?}");
    assert!(names.contains(&"engine.list_tools"), "{names:?}");

    let unknown = server.request(
        3,
        "tools/call",
        json!({ "name": "no.such_tool", "arguments": {} }),
    );
    let message = unknown["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("expected a JSON-RPC error, got {unknown}"));
    assert!(message.contains("no.such_tool"), "{message}");

    // The server keeps serving after the error.
    let info = server.request(
        4,
        "tools/call",
        json!({ "name": "engine.info", "arguments": {} }),
    );
    let info: Value = serde_json::from_str(first_text(&info)).unwrap();
    assert!(info.get("entities").is_some(), "{info}");
    assert_eq!(info["target"], json!({ "width": 64, "height": 36 }));

    let lines = server.shutdown();
    assert!(lines.len() >= 4, "{lines:?}");
    for line in &lines {
        parse_json_line(line);
    }
}
