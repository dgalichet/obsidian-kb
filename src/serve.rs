use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use crate::benchmark;
use crate::cli::ServeArgs;
use crate::config::AppConfig;
use crate::db::Db;
use crate::indexer::{self, IndexOptions};
use crate::mcp_server::{MCP_PROTOCOL_VERSION, McpServer};
use crate::paths::KbPaths;

const IDLE_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

/// Runs the local HTTP service for REST and streamable HTTP MCP access.
pub fn run(mut config: AppConfig, args: ServeArgs) -> Result<()> {
    if let Some(vault) = args.vault.as_deref() {
        config.vault.path = std::fs::canonicalize(vault)?;
    }
    if let Some(seconds) = args.idle_unload_seconds {
        config.mcp.idle_unload_seconds = seconds;
    }
    if args.preload_embedder {
        config.mcp.preload_embedder = true;
    }

    let listener = TcpListener::bind(("127.0.0.1", args.port))
        .with_context(|| format!("failed to bind 127.0.0.1:{}", args.port))?;
    listener.set_nonblocking(true)?;
    let local_addr = listener.local_addr()?;

    let mut server = McpServer::new(config);
    if server.config().mcp.preload_embedder && server.config().embeddings.enabled {
        server.warm_up_vector_cache()?;
    }

    println!("obsidian-kb serve listening on {}", base_url(local_addr));
    println!("mcp endpoint: {}/mcp", base_url(local_addr));
    std::io::stdout().flush().ok();

    let mut shutdown = false;
    while !shutdown {
        server.unload_idle_vector_cache();
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                stream.set_read_timeout(Some(Duration::from_secs(30))).ok();
                stream.set_write_timeout(Some(Duration::from_secs(30))).ok();
                let response = handle_connection(&mut server, &mut stream);
                shutdown = response.shutdown;
                if let Err(error) = write_http_response(&mut stream, response)
                    && !is_client_disconnect(&error)
                {
                    return Err(error);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(IDLE_CHECK_INTERVAL);
            }
            Err(error) => return Err(error.into()),
        }
    }

    Ok(())
}

fn handle_connection(server: &mut McpServer, stream: &mut TcpStream) -> HttpResponse {
    match read_http_request(stream) {
        Ok(Some(request)) => route_request(server, request),
        Ok(None) => HttpResponse::empty(400),
        Err(error) => HttpResponse::json_error(400, error.to_string()),
    }
}

fn route_request(server: &mut McpServer, request: HttpRequest) -> HttpResponse {
    server.unload_idle_vector_cache();
    let path = request.target_path();
    let cors_origin = match cors_origin(server.config(), &request) {
        Ok(origin) => origin,
        Err(response) => return response,
    };

    let mut response = if request.method == "OPTIONS" {
        HttpResponse::empty(204)
    } else {
        match (request.method.as_str(), path) {
            ("GET", "/health") => HttpResponse::json(
                200,
                json!({
                    "ok": true,
                    "version": crate::version::version()
                }),
            ),
            ("GET", "/status") => HttpResponse::json(200, status_json(server)),
            ("POST", "/search") => route_json_body(&request, |body| server.tool_search_http(&body)),
            ("POST", "/show") => route_json_body(&request, |body| server.tool_show(&body)),
            ("POST", "/graph") => route_json_body(&request, |body| server.tool_graph(&body)),
            ("POST", "/index/refresh") => route_index_refresh(server, &request),
            ("POST", "/shutdown") => {
                let mut response = HttpResponse::json(200, json!({ "shutting_down": true }));
                response.shutdown = true;
                response
            }
            ("POST", "/mcp") => route_mcp(server, &request),
            _ if matches!(path, "/health" | "/status") => HttpResponse::json_error(
                405,
                format!("method {} is not allowed for {path}", request.method),
            ),
            _ if matches!(
                path,
                "/search" | "/show" | "/graph" | "/index/refresh" | "/shutdown" | "/mcp"
            ) =>
            {
                HttpResponse::json_error(
                    405,
                    format!("method {} is not allowed for {path}", request.method),
                )
            }
            _ => HttpResponse::json_error(404, format!("unknown endpoint `{path}`")),
        }
    };

    add_cors_headers(&mut response, cors_origin);
    response
}

fn cors_origin(
    config: &AppConfig,
    request: &HttpRequest,
) -> std::result::Result<Option<String>, HttpResponse> {
    let Some(origin) = request.headers.get("origin") else {
        return Ok(None);
    };
    allowed_cors_origin(&config.serve.cors_allowed_origins, origin)
        .map(Some)
        .ok_or_else(|| HttpResponse::json_error(403, format!("origin `{origin}` is not allowed")))
}

fn allowed_cors_origin(allowed_origins: &[String], origin: &str) -> Option<String> {
    let mut allow_any = false;
    for allowed_origin in allowed_origins {
        let allowed_origin = allowed_origin.trim();
        if allowed_origin == "*" {
            allow_any = true;
        } else if allowed_origin == origin {
            return Some(origin.to_string());
        }
    }
    allow_any.then(|| "*".to_string())
}

fn add_cors_headers(response: &mut HttpResponse, origin: Option<String>) {
    let Some(origin) = origin else {
        return;
    };

    response
        .extra_headers
        .push(("Access-Control-Allow-Origin", origin));
    response.extra_headers.push(("Vary", "Origin".to_string()));
    response.extra_headers.push((
        "Access-Control-Allow-Headers",
        "content-type, accept, mcp-protocol-version".to_string(),
    ));
    response.extra_headers.push((
        "Access-Control-Allow-Methods",
        "GET, POST, OPTIONS".to_string(),
    ));
}

fn route_json_body(
    request: &HttpRequest,
    operation: impl FnOnce(Value) -> Result<Value>,
) -> HttpResponse {
    match json_body(request).and_then(operation) {
        Ok(value) => HttpResponse::json(200, value),
        Err(error) => HttpResponse::json_error(400, error.to_string()),
    }
}

fn route_index_refresh(server: &mut McpServer, request: &HttpRequest) -> HttpResponse {
    let body = match json_body(request) {
        Ok(body) => body,
        Err(error) => return HttpResponse::json_error(400, error.to_string()),
    };
    let options = match index_options(&body) {
        Ok(options) => options,
        Err(error) => return HttpResponse::json_error(400, error.to_string()),
    };
    let config = server.config().clone();
    let result = benchmark::measure(
        &config,
        "serve_index_refresh",
        |benchmark| {
            benchmark.set_field("rebuild", options.rebuild);
            benchmark.set_field("changed_only", options.changed_only);
            benchmark.set_field("no_embeddings", options.no_embeddings);
        },
        |benchmark| indexer::refresh_with_benchmark(&config, options, benchmark),
    );
    match result {
        Ok(outcome) => {
            server.unload_vector_cache();
            HttpResponse::json(200, serde_json::to_value(outcome).unwrap_or(Value::Null))
        }
        Err(error) => HttpResponse::json_error(500, error.to_string()),
    }
}

fn route_mcp(server: &mut McpServer, request: &HttpRequest) -> HttpResponse {
    let body = match std::str::from_utf8(&request.body) {
        Ok(body) => body,
        Err(error) => {
            return HttpResponse::json_error(400, format!("request body is not UTF-8: {error}"));
        }
    };
    let Some(response) = server.handle_http_message(body) else {
        let mut response = HttpResponse::empty(202);
        response
            .extra_headers
            .push(("MCP-Protocol-Version", MCP_PROTOCOL_VERSION.to_string()));
        return response;
    };

    let mut response = if request.accepts("text/event-stream") {
        HttpResponse::text(
            200,
            "text/event-stream",
            format!("event: message\ndata: {response}\n\n"),
        )
    } else {
        HttpResponse::text(200, "application/json", response)
    };
    response
        .extra_headers
        .push(("MCP-Protocol-Version", MCP_PROTOCOL_VERSION.to_string()));
    response
}

fn status_json(server: &McpServer) -> Value {
    let config = server.config();
    let paths = KbPaths::from_config(config);
    let (index_available, stats, index_error) = if paths.db_path.exists() {
        match Db::open(&paths.db_path).and_then(|db| db.stats()) {
            Ok(stats) => (true, Some(stats), None),
            Err(error) => (false, None, Some(error.to_string())),
        }
    } else {
        (false, None, None)
    };

    json!({
        "ok": true,
        "version": crate::version::version(),
        "vault_path": config.vault.path,
        "index_dir": paths.index_dir,
        "database_path": paths.db_path,
        "tantivy_index_dir": paths.tantivy_dir,
        "index": {
            "available": index_available,
            "stats": stats,
            "error": index_error
        },
        "mcp": server.tool_status()
    })
}

fn json_body(request: &HttpRequest) -> Result<Value> {
    if request.body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(&request.body).context("request body must be JSON")
}

fn index_options(body: &Value) -> Result<IndexOptions> {
    Ok(IndexOptions {
        rebuild: bool_field(body, "rebuild", false)?,
        changed_only: bool_field(body, "changed_only", false)?,
        no_embeddings: bool_field(body, "no_embeddings", false)?,
    })
}

fn bool_field(body: &Value, name: &str, default: bool) -> Result<bool> {
    body.get(name)
        .map(|value| {
            value
                .as_bool()
                .with_context(|| format!("`{name}` must be a boolean"))
        })
        .unwrap_or(Ok(default))
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn target_path(&self) -> &str {
        self.target
            .split_once('?')
            .map(|(path, _query)| path)
            .unwrap_or(&self.target)
    }

    fn accepts(&self, content_type: &str) -> bool {
        self.headers
            .get("accept")
            .map(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().starts_with(content_type))
            })
            .unwrap_or(false)
    }
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    content_type: Option<&'static str>,
    body: Vec<u8>,
    shutdown: bool,
    extra_headers: Vec<(&'static str, String)>,
}

impl HttpResponse {
    fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: None,
            body: Vec::new(),
            shutdown: false,
            extra_headers: Vec::new(),
        }
    }

    fn json(status: u16, value: Value) -> Self {
        let body = serde_json::to_vec(&value).unwrap_or_else(|_| b"null".to_vec());
        Self {
            status,
            content_type: Some("application/json"),
            body,
            shutdown: false,
            extra_headers: Vec::new(),
        }
    }

    fn json_error(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, json!({ "error": { "message": message.into() } }))
    }

    fn text(status: u16, content_type: &'static str, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: Some(content_type),
            body: body.into().into_bytes(),
            shutdown: false,
            extra_headers: Vec::new(),
        }
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line)? == 0 {
        return Ok(None);
    }
    if first_line.trim().is_empty() {
        return Ok(None);
    }
    let mut request_parts = first_line.split_whitespace();
    let method = request_parts
        .next()
        .map(str::to_ascii_uppercase)
        .context("missing HTTP method")?;
    let target = request_parts
        .next()
        .map(ToOwned::to_owned)
        .context("missing HTTP target")?;
    let version = request_parts.next().context("missing HTTP version")?;
    if !version.starts_with("HTTP/") {
        bail!("unsupported HTTP version `{version}`");
    }

    let mut headers = BTreeMap::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            bail!("malformed HTTP header `{line}`");
        };
        headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().context("invalid Content-Length"))
        .transpose()?
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        bail!("request body exceeds {MAX_BODY_BYTES} bytes");
    }
    let mut body = vec![0; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Some(HttpRequest {
        method,
        target,
        headers,
        body,
    }))
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason_phrase(response.status)
    )?;
    if let Some(content_type) = response.content_type {
        write!(stream, "Content-Type: {content_type}\r\n")?;
    }
    write!(stream, "Content-Length: {}\r\n", response.body.len())?;
    write!(stream, "Connection: close\r\n")?;
    for (name, value) in response.extra_headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(&response.body)?;
    stream.flush()?;
    Ok(())
}

fn is_client_disconnect(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .map(|error| {
            matches!(
                error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
            )
        })
        .unwrap_or(false)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn base_url(addr: SocketAddr) -> String {
    if addr.ip().is_ipv6() {
        format!("http://[{}]:{}", addr.ip(), addr.port())
    } else {
        format!("http://{}:{}", addr.ip(), addr.port())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn routes_health_search_mcp_and_shutdown() {
        let (_temp, config) = indexed_test_config();
        let mut server = McpServer::new(config);

        let health = route_request(&mut server, request("GET", "/health", None));
        assert_eq!(health.status, 200);
        assert_eq!(json_response(&health)["ok"], true);

        let search = route_request(
            &mut server,
            request(
                "POST",
                "/search",
                Some(json!({ "query": "Alpha", "mode": "bm25", "top": 1 })),
            ),
        );
        assert_eq!(search.status, 200);
        assert_eq!(json_response(&search)[0]["path"], "alpha.md");

        let mcp = route_request(
            &mut server,
            request(
                "POST",
                "/mcp",
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": { "protocolVersion": MCP_PROTOCOL_VERSION }
                })),
            ),
        );
        assert_eq!(mcp.status, 200);
        assert_eq!(
            json_response(&mcp)["result"]["serverInfo"]["name"],
            "obsidian-kb"
        );

        let refresh = route_request(
            &mut server,
            request(
                "POST",
                "/index/refresh",
                Some(json!({ "no_embeddings": true })),
            ),
        );
        assert_eq!(refresh.status, 200);
        assert_eq!(json_response(&refresh)["stats"]["notes"], 2);

        let shutdown = route_request(&mut server, request("POST", "/shutdown", None));
        assert_eq!(shutdown.status, 200);
        assert!(shutdown.shutdown);
    }

    #[test]
    fn http_search_benchmark_records_http_transport() {
        let (_temp, mut config) = indexed_test_config();
        config.benchmark.enabled = true;
        let log_path = config.benchmark_log_path();
        let mut server = McpServer::new(config);

        let search = route_request(
            &mut server,
            request(
                "POST",
                "/search",
                Some(json!({ "query": "Alpha", "mode": "bm25", "top": 1 })),
            ),
        );

        assert_eq!(search.status, 200);
        let content = std::fs::read_to_string(log_path).unwrap();
        let record: Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();
        assert_eq!(record["command"], "http_search");
        assert_eq!(record["transport"], "http_rest");
    }

    #[test]
    fn streamable_mcp_search_benchmark_records_http_transport() {
        let (_temp, mut config) = indexed_test_config();
        config.benchmark.enabled = true;
        let log_path = config.benchmark_log_path();
        let mut server = McpServer::new(config);

        let response = route_request(
            &mut server,
            request(
                "POST",
                "/mcp",
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "tools/call",
                    "params": {
                        "name": "search",
                        "arguments": { "query": "Alpha", "mode": "bm25", "top": 1 }
                    }
                })),
            ),
        );

        assert_eq!(response.status, 200);
        let content = std::fs::read_to_string(log_path).unwrap();
        let record: Value = serde_json::from_str(content.lines().last().unwrap()).unwrap();
        assert_eq!(record["command"], "mcp_search");
        assert_eq!(record["transport"], "mcp_http");
    }

    #[test]
    fn mcp_can_return_sse_for_streamable_http_clients() {
        let (_temp, config) = indexed_test_config();
        let mut server = McpServer::new(config);
        let mut request = request(
            "POST",
            "/mcp",
            Some(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "ping"
            })),
        );
        request
            .headers
            .insert("accept".to_string(), "text/event-stream".to_string());

        let response = route_request(&mut server, request);

        assert_eq!(response.status, 200);
        assert_eq!(response.content_type, Some("text/event-stream"));
        assert!(
            String::from_utf8(response.body)
                .unwrap()
                .starts_with("event: message\n")
        );
    }

    #[test]
    fn cors_allows_configured_obsidian_origin() {
        let (_temp, config) = indexed_test_config();
        let mut server = McpServer::new(config);

        let health = route_request(
            &mut server,
            request_with_origin("GET", "/health", None, "app://obsidian.md"),
        );

        assert_eq!(health.status, 200);
        assert_eq!(
            response_header(&health, "Access-Control-Allow-Origin"),
            Some("app://obsidian.md")
        );
        assert_eq!(response_header(&health, "Vary"), Some("Origin"));
    }

    #[test]
    fn cors_rejects_unconfigured_browser_origins_before_side_effects() {
        let (_temp, config) = indexed_test_config();
        let mut server = McpServer::new(config);

        let shutdown = route_request(
            &mut server,
            request_with_origin("POST", "/shutdown", None, "https://example.com"),
        );

        assert_eq!(shutdown.status, 403);
        assert!(!shutdown.shutdown);
        assert_eq!(
            response_header(&shutdown, "Access-Control-Allow-Origin"),
            None
        );
    }

    #[test]
    fn client_disconnect_write_errors_do_not_stop_server() {
        assert!(is_client_disconnect(
            &std::io::Error::from(std::io::ErrorKind::BrokenPipe).into()
        ));
        assert!(is_client_disconnect(
            &std::io::Error::from(std::io::ErrorKind::ConnectionReset).into()
        ));
        assert!(!is_client_disconnect(
            &std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()
        ));
    }

    fn request(method: &str, target: &str, body: Option<Value>) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            target: target.to_string(),
            headers: BTreeMap::new(),
            body: body
                .map(|value| serde_json::to_vec(&value).unwrap())
                .unwrap_or_default(),
        }
    }

    fn request_with_origin(
        method: &str,
        target: &str,
        body: Option<Value>,
        origin: &str,
    ) -> HttpRequest {
        let mut request = request(method, target, body);
        request
            .headers
            .insert("origin".to_string(), origin.to_string());
        request
    }

    fn json_response(response: &HttpResponse) -> Value {
        serde_json::from_slice(&response.body).unwrap()
    }

    fn response_header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
        response
            .extra_headers
            .iter()
            .find(|(header_name, _value)| *header_name == name)
            .map(|(_header_name, value)| value.as_str())
    }

    fn indexed_test_config() -> (TempDir, AppConfig) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(
            vault.join("alpha.md"),
            r#"---
tags: [research]
status: active
---
# Alpha

Alpha retrieval note linking to [[Beta]].
"#,
        )
        .unwrap();
        std::fs::write(
            vault.join("beta.md"),
            r#"---
tags: [business]
status: done
---
# Beta

Beta planning note.
"#,
        )
        .unwrap();

        let mut config = AppConfig::default_for_vault_in(&vault, None, temp.path()).unwrap();
        config.embeddings.enabled = false;
        config.search.default_mode = "bm25".to_string();
        indexer::refresh_with_benchmark(
            &config,
            IndexOptions {
                no_embeddings: true,
                ..IndexOptions::default()
            },
            None,
        )
        .unwrap();

        (temp, config)
    }
}
