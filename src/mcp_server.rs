use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use crate::benchmark;
use crate::cli::McpArgs;
use crate::config::AppConfig;
use crate::db::Db;
use crate::models::{SearchFilters, SearchMode};
use crate::paths::KbPaths;
use crate::search::{self, SearchOptions};
use crate::vector_search::VectorSearchCache;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const IDLE_CHECK_INTERVAL: Duration = Duration::from_millis(500);

/// Runs the obsidian-kb MCP server over stdio.
pub fn run(mut config: AppConfig, args: McpArgs) -> Result<()> {
    if let Some(vault) = args.vault.as_deref() {
        config.vault.path = std::fs::canonicalize(vault)?;
    }
    if let Some(seconds) = args.idle_unload_seconds {
        config.mcp.idle_unload_seconds = seconds;
    }
    if args.preload_embedder {
        config.mcp.preload_embedder = true;
    }

    let mut server = McpServer::new(config);
    if server.config.mcp.preload_embedder && server.config.embeddings.enabled {
        server.warm_up_vector_cache()?;
    }

    let (messages, reader) = mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut stdin = BufReader::new(stdin);
        loop {
            match read_message(&mut stdin) {
                Ok(Some(message)) => {
                    if messages.send(Ok(message)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = messages.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });

    let stdout = std::io::stdout();
    let mut stdout = BufWriter::new(stdout.lock());
    loop {
        server.unload_idle_vector_cache();
        match reader.recv_timeout(IDLE_CHECK_INTERVAL) {
            Ok(Ok(message)) => {
                if let Some(response) = server.handle_message(&message) {
                    write_message(&mut stdout, &response)?;
                }
            }
            Ok(Err(error)) => {
                let response = error_response(Value::Null, -32700, &error);
                write_message(&mut stdout, &response)?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}

struct McpServer {
    config: AppConfig,
    vector_cache: VectorSearchCache,
}

impl McpServer {
    fn new(config: AppConfig) -> Self {
        Self {
            config,
            vector_cache: VectorSearchCache::default(),
        }
    }

    fn warm_up_vector_cache(&mut self) -> Result<()> {
        self.vector_cache.warm_up(&self.config)
    }

    fn unload_idle_vector_cache(&mut self) {
        let seconds = self.config.mcp.idle_unload_seconds;
        if seconds == 0 {
            return;
        }
        self.vector_cache
            .unload_if_idle(Duration::from_secs(seconds));
    }

    fn handle_message(&mut self, message: &str) -> Option<String> {
        let parsed = match serde_json::from_str::<Value>(message) {
            Ok(parsed) => parsed,
            Err(error) => {
                return Some(error_response(
                    Value::Null,
                    -32700,
                    &format!("invalid JSON-RPC message: {error}"),
                ));
            }
        };
        if let Some(batch) = parsed.as_array() {
            let responses = batch
                .iter()
                .filter_map(|request| self.handle_request(request))
                .collect::<Vec<_>>();
            return (!responses.is_empty()).then(|| Value::Array(responses).to_string());
        }
        self.handle_request(&parsed)
            .map(|response| response.to_string())
    }

    fn handle_request(&mut self, request: &Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str);
        let Some(method) = method else {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id.unwrap_or(Value::Null),
                "error": json_rpc_error(-32600, "missing JSON-RPC method")
            }));
        };
        if id.is_none() && method.starts_with("notifications/") {
            return None;
        }
        let id = id.unwrap_or(Value::Null);
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let result = match method {
            "initialize" => Ok(self.initialize_result(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => self.call_tool(&params),
            _ => Err(json_rpc_error(-32601, format!("unknown method `{method}`"))),
        };

        Some(match result {
            Ok(result) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }),
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": error,
            }),
        })
    }

    fn initialize_result(&self, params: &Value) -> Value {
        let protocol_version = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(MCP_PROTOCOL_VERSION);
        json!({
            "protocolVersion": protocol_version,
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "obsidian-kb",
                "version": crate::version::version()
            }
        })
    }

    fn call_tool(&mut self, params: &Value) -> std::result::Result<Value, Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| json_rpc_error(-32602, "tools/call requires params.name"))?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let result = match name {
            "search" => self.tool_search(&arguments),
            "show" => self.tool_show(&arguments),
            "stats" => self.tool_stats(),
            "warmup" => self.tool_warmup(),
            "unload" => Ok(json!({
                "unloaded": self.vector_cache.unload(),
                "vector_embedder_loaded": self.vector_cache.is_loaded()
            })),
            "status" => Ok(json!({
                "vector_embedder_loaded": self.vector_cache.is_loaded(),
                "idle_unload_seconds": self.config.mcp.idle_unload_seconds
            })),
            _ => Err(anyhow!("unknown tool `{name}`")),
        };
        match result {
            Ok(value) => Ok(tool_result(value)),
            Err(error) => Ok(json!({
                "content": [{
                    "type": "text",
                    "text": error.to_string()
                }],
                "isError": true
            })),
        }
    }

    fn tool_search(&mut self, arguments: &Value) -> Result<Value> {
        let query = optional_string_arg(arguments, "query").unwrap_or("");
        let mode = optional_string_arg(arguments, "mode")
            .and_then(SearchMode::from_config_value)
            .unwrap_or_else(|| self.config.default_search_mode());
        let top = usize_arg(arguments, "top", 10)?;
        let expand_graph = bool_arg(arguments, "expand_graph", false)?;
        let include_text = bool_arg(arguments, "include_text", false)?;
        let max_chars = usize_arg(arguments, "max_chars", 1200)?;
        let filters = SearchFilters {
            tags: string_array_arg(arguments, "tags")?,
            properties: string_array_arg(arguments, "properties")?
                .iter()
                .map(|value| search::parse_property_filter(value))
                .collect::<Result<Vec<_>>>()?,
        };
        let options = SearchOptions {
            mode,
            limit: top,
            graph: expand_graph,
            include_text,
            max_chars,
            filters: filters.clone(),
        };
        let config = self.config.clone();
        let vector_cache = &mut self.vector_cache;
        let hits = benchmark::measure(
            &config,
            "mcp_search",
            |benchmark| {
                benchmark.set_field("mode", search_mode_name(mode));
                benchmark.set_field("top", top);
                benchmark.set_field("expand_graph", expand_graph);
                benchmark.set_field("include_text", include_text);
                benchmark.set_field("max_chars", max_chars);
                benchmark.set_field("tag_filters", filters.tags.len());
                benchmark.set_field("property_filters", filters.properties.len());
                benchmark.set_field("query_chars", query.chars().count());
                if config.benchmark.include_query {
                    benchmark.set_field("query", query);
                }
            },
            |benchmark| {
                search::search_with_vector_cache(
                    &config,
                    query,
                    options,
                    benchmark,
                    Some(vector_cache),
                )
            },
        )?;
        serde_json::to_value(hits).map_err(Into::into)
    }

    fn tool_show(&self, arguments: &Value) -> Result<Value> {
        let chunk_id = string_arg(arguments, "chunk_id")?;
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        let chunk = db
            .load_chunk(chunk_id)?
            .with_context(|| format!("chunk not found: {chunk_id}"))?;
        serde_json::to_value(chunk).map_err(Into::into)
    }

    fn tool_stats(&self) -> Result<Value> {
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        serde_json::to_value(db.stats()?).map_err(Into::into)
    }

    fn tool_warmup(&mut self) -> Result<Value> {
        self.vector_cache.warm_up(&self.config)?;
        Ok(json!({
            "vector_embedder_loaded": self.vector_cache.is_loaded()
        }))
    }
}

fn read_message(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        if !line.trim().is_empty() {
            break;
        }
    }

    if line.trim_start().starts_with('{') || line.trim_start().starts_with('[') {
        return Ok(Some(line.trim().to_string()));
    }

    let mut content_length = None;
    loop {
        let header = line.trim_end_matches(['\r', '\n']);
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = Some(value.trim().parse::<usize>()?);
        }

        line.clear();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
    }

    let length = content_length.context("missing Content-Length header")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    String::from_utf8(body).map(Some).map_err(Into::into)
}

fn write_message(writer: &mut impl Write, message: &str) -> Result<()> {
    writer.write_all(message.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn tools() -> Value {
    json!([
        {
            "name": "search",
            "description": "Search the indexed Obsidian vault. Hybrid and vector searches reuse a warm local embedding model while it remains loaded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "mode": { "type": "string", "enum": ["bm25", "vector", "hybrid"] },
                    "top": { "type": "integer", "minimum": 0 },
                    "expand_graph": { "type": "boolean" },
                    "include_text": { "type": "boolean" },
                    "max_chars": { "type": "integer", "minimum": 0 },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "properties": {
                        "type": "array",
                        "items": { "type": "string", "description": "KEY=VALUE, KEY!=VALUE, KEY>=VALUE, KEY<=VALUE, KEY>VALUE, or KEY<VALUE" }
                    }
                }
            }
        },
        {
            "name": "show",
            "description": "Load one indexed chunk by chunk id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chunk_id": { "type": "string" }
                },
                "required": ["chunk_id"]
            }
        },
        {
            "name": "stats",
            "description": "Return index statistics.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "warmup",
            "description": "Initialize and keep the local embedding model warm for subsequent vector searches.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "unload",
            "description": "Unload the cached local embedding model immediately.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "status",
            "description": "Return MCP server cache status.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

fn tool_result(value: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        }],
        "isError": false
    })
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": json_rpc_error(code, message)
    })
    .to_string()
}

fn json_rpc_error(code: i64, message: impl ToString) -> Value {
    json!({
        "code": code,
        "message": message.to_string()
    })
}

fn string_arg<'a>(arguments: &'a Value, name: &str) -> Result<&'a str> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string argument `{name}`"))
}

fn optional_string_arg<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

fn string_array_arg(arguments: &Value, name: &str) -> Result<Vec<String>> {
    let Some(value) = arguments.get(name) else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .with_context(|| format!("argument `{name}` must be an array of strings"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .with_context(|| format!("argument `{name}` must contain only strings"))
        })
        .collect()
}

fn bool_arg(arguments: &Value, name: &str, default: bool) -> Result<bool> {
    arguments
        .get(name)
        .map(|value| {
            value
                .as_bool()
                .with_context(|| format!("argument `{name}` must be a boolean"))
        })
        .unwrap_or(Ok(default))
}

fn usize_arg(arguments: &Value, name: &str, default: usize) -> Result<usize> {
    arguments
        .get(name)
        .map(|value| {
            let value = value
                .as_u64()
                .with_context(|| format!("argument `{name}` must be an unsigned integer"))?;
            usize::try_from(value).with_context(|| format!("argument `{name}` is too large"))
        })
        .unwrap_or(Ok(default))
}

fn search_mode_name(mode: SearchMode) -> &'static str {
    match mode {
        SearchMode::Bm25 => "bm25",
        SearchMode::Vector => "vector",
        SearchMode::Hybrid => "hybrid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn reads_content_length_framed_message() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        let mut cursor = Cursor::new(message.into_bytes());

        assert_eq!(read_message(&mut cursor).unwrap().unwrap(), body);
    }

    #[test]
    fn writes_json_line_message() {
        let mut output = Vec::new();

        write_message(&mut output, r#"{"ok":true}"#).unwrap();

        assert_eq!(String::from_utf8(output).unwrap(), "{\"ok\":true}\n");
    }
}
