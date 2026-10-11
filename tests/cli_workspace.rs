use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Service(Child);

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(vault: &Path) -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("obsidian-kb"));
    command.current_dir(vault);
    command.arg("--config").arg(vault.join(".obsidian-kb.toml"));
    command
}

fn start_service(vault: &Path) -> (Service, u16) {
    let mut child = command(vault)
        .args(["serve", "--port", "0"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let port = line.trim().rsplit(':').next().unwrap().parse().unwrap();
    (Service(child), port)
}

fn http(port: u16, method: &str, path: &str, body: Value) -> (u16, Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let body = body.to_string();
    write!(stream, "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, serde_json::from_str(body).unwrap())
}

fn stdio_current(vault: &Path, port: u16, arguments: Value) -> Value {
    let mut child = command(vault)
        .args(["mcp", "--workspace-port", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": "get_current_note", "arguments": arguments } });
    writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let structured = reply["result"]["structuredContent"].clone();
    let text: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(structured, text);
    structured
}

fn vault(temp: &Path, name: &str) -> PathBuf {
    let vault = temp.join(name);
    std::fs::create_dir_all(&vault).unwrap();
    let output = Command::new(assert_cmd::cargo::cargo_bin!("obsidian-kb"))
        .current_dir(&vault)
        .arg("init")
        .arg("--vault")
        .arg(&vault)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(vault.join(".obsidian-kb.toml").is_file());
    assert!(vault.join(".obsidian-kb/metadata.sqlite").is_file());
    assert!(!temp.join(".obsidian-kb.toml").exists());
    assert!(!temp.join(".obsidian-kb").exists());
    std::fs::write(vault.join("Unindexed.md"), "# Unindexed note").unwrap();
    vault.canonicalize().unwrap()
}

#[test]
fn workspace_publication_is_shared_with_http_and_stdio_mcp_and_never_crosses_vaults() {
    let temp = tempfile::tempdir().unwrap();
    let vault = vault(temp.path(), "vault");
    let (_service, port) = start_service(&vault);
    let context = json!({"vault_path":vault, "session_id":"window-1", "context_version":1,
        "note":{"path":"Unindexed.md","title":"Unindexed"}});
    assert_eq!(
        http(port, "POST", "/workspace/context", context.clone()).0,
        200
    );
    let http_mcp = http(
        port,
        "POST",
        "/mcp",
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
        "params":{"name":"get_current_note","arguments":{}}}),
    );
    assert_eq!(http_mcp.0, 200);
    assert_eq!(
        http_mcp.1["result"]["structuredContent"]["note"]["path"],
        "Unindexed.md"
    );
    let reply = stdio_current(&vault, port, json!({}));
    assert_eq!(reply["status"], "available");
    assert_eq!(reply["note"]["path"], "Unindexed.md");
    assert_eq!(reply["vault_path"], vault.to_str().unwrap());

    let mut invalid = context.clone();
    invalid["context_version"] = json!(0);
    assert_eq!(http(port, "POST", "/workspace/context", invalid).0, 400);
    assert_eq!(
        http(
            port,
            "POST",
            "/workspace/context",
            json!({"vault_path":vault, "session_id":"window-1", "context_version":1,"note":null})
        )
        .0,
        400
    );
    let other = self::vault(temp.path(), "other");
    assert_eq!(
        http(
            port,
            "POST",
            "/workspace/current-note",
            json!({"vault_path":other})
        )
        .0,
        400
    );
    let reply = stdio_current(&other, port, json!({}));
    assert_eq!(reply["status"], "unavailable");
    assert!(reply["note"].is_null());

    assert_eq!(
        http(
            port,
            "POST",
            "/workspace/context",
            json!({"vault_path":vault, "session_id":"window-2", "context_version":1,"note":null})
        )
        .0,
        200
    );
    assert_eq!(
        stdio_current(&vault, port, json!({}))["status"],
        "ambiguous"
    );
    assert_eq!(
        stdio_current(&vault, port, json!({"session_id":"window-1"}))["status"],
        "available"
    );
    assert_eq!(
        http(
            port,
            "DELETE",
            "/workspace/context",
            json!({"vault_path":vault, "session_id":"window-1"})
        )
        .0,
        200
    );
    assert_eq!(
        stdio_current(&vault, port, json!({}))["status"],
        "no_active_note"
    );
}
