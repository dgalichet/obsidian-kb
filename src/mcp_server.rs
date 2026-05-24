use anyhow::{Context, Result, anyhow, bail};
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
use crate::related::{self, RelatedInput, RelatedOptions};
use crate::search::{self, SearchOptions};
use crate::vector_search::VectorSearchCache;

pub(crate) const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
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

pub(crate) struct McpServer {
    config: AppConfig,
    transport: &'static str,
    vector_cache: VectorSearchCache,
}

impl McpServer {
    pub(crate) fn new(config: AppConfig) -> Self {
        Self {
            config,
            transport: "mcp_stdio",
            vector_cache: VectorSearchCache::default(),
        }
    }

    pub(crate) fn config(&self) -> &AppConfig {
        &self.config
    }

    pub(crate) fn vector_embedder_loaded(&self) -> bool {
        self.vector_cache.is_loaded()
    }

    pub(crate) fn warm_up_vector_cache(&mut self) -> Result<()> {
        self.vector_cache.warm_up(&self.config)?;
        if self.config.embeddings.enabled {
            let paths = KbPaths::from_config(&self.config);
            if paths.db_path.exists() {
                let db = Db::open(&paths.db_path)?;
                self.vector_cache.warm_up_embeddings(&db, &self.config)?;
            }
        }
        Ok(())
    }

    pub(crate) fn unload_vector_cache(&mut self) -> bool {
        self.vector_cache.unload()
    }

    pub(crate) fn unload_idle_vector_cache(&mut self) {
        let seconds = self.config.mcp.idle_unload_seconds;
        if seconds == 0 {
            return;
        }
        self.vector_cache
            .unload_if_idle(Duration::from_secs(seconds));
    }

    pub(crate) fn handle_message(&mut self, message: &str) -> Option<String> {
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
        self.handle_value(&parsed)
            .map(|response| response.to_string())
    }

    pub(crate) fn handle_http_message(&mut self, message: &str) -> Option<String> {
        let previous = self.transport;
        self.transport = "mcp_http";
        let response = self.handle_message(message);
        self.transport = previous;
        response
    }

    pub(crate) fn handle_value(&mut self, parsed: &Value) -> Option<Value> {
        if let Some(batch) = parsed.as_array() {
            let responses = batch
                .iter()
                .filter_map(|request| self.handle_request(request))
                .collect::<Vec<_>>();
            return (!responses.is_empty()).then_some(Value::Array(responses));
        }
        self.handle_request(parsed)
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
            "related" => self.tool_related(&arguments),
            "show" => self.tool_show(&arguments),
            "graph" => self.tool_graph(&arguments),
            "tags" => self.tool_tags(&arguments),
            "properties" => self.tool_properties(&arguments),
            "stats" => self.tool_stats(),
            "warmup" => self.tool_warmup(),
            "unload" => Ok(json!({
                "unloaded": self.vector_cache.unload(),
                "vector_embedder_loaded": self.vector_cache.is_loaded(),
                "vector_embeddings_loaded": self.vector_cache.embeddings_loaded(),
                "vector_embedding_count": self.vector_cache.cached_embedding_count()
            })),
            "status" => Ok(self.tool_status()),
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

    pub(crate) fn tool_search(&mut self, arguments: &Value) -> Result<Value> {
        self.tool_search_with_benchmark(arguments, "mcp_search", self.transport)
    }

    pub(crate) fn tool_search_http(&mut self, arguments: &Value) -> Result<Value> {
        self.tool_search_with_benchmark(arguments, "http_search", "http_rest")
    }

    fn tool_search_with_benchmark(
        &mut self,
        arguments: &Value,
        command: &'static str,
        transport: &'static str,
    ) -> Result<Value> {
        let query = optional_string_arg(arguments, "query").unwrap_or("");
        let mode = optional_string_arg(arguments, "mode")
            .and_then(SearchMode::from_config_value)
            .unwrap_or_else(|| self.config.default_search_mode());
        let top = usize_arg(arguments, "top", 10)?;
        let expand_graph = bool_arg(arguments, "expand_graph", false)?;
        let include_text = bool_arg(arguments, "include_text", false)?;
        let max_chars = usize_arg(arguments, "max_chars", 1200)?;
        let mut tags = string_list_arg(arguments, "tags")?;
        tags.extend(string_list_arg(arguments, "tag")?);
        let mut properties = string_list_arg(arguments, "properties")?;
        properties.extend(string_list_arg(arguments, "property")?);
        let filters = SearchFilters {
            tags,
            properties: properties
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
            command,
            |benchmark| {
                benchmark.set_field("transport", transport);
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

    pub(crate) fn tool_related(&mut self, arguments: &Value) -> Result<Value> {
        let note = optional_string_arg(arguments, "note");
        let text = optional_string_arg(arguments, "text");
        let source_count = if note.is_some() { 1 } else { 0 } + if text.is_some() { 1 } else { 0 };
        if source_count != 1 {
            bail!("related requires exactly one source: note or text");
        }
        let input = if let Some(note) = note {
            RelatedInput::Note(note.to_string())
        } else {
            RelatedInput::Text(text.unwrap_or_default().to_string())
        };
        let top = usize_arg(arguments, "top", 10)?;
        let candidates = usize_arg(arguments, "candidates", 0)?;
        let config = self.config.clone();
        let transport = self.transport;
        let vector_cache = &mut self.vector_cache;
        let report = benchmark::measure(
            &config,
            "mcp_related",
            |benchmark| {
                benchmark.set_field("transport", transport);
                benchmark.set_field("top", top);
                benchmark.set_field("candidates", candidates);
                match &input {
                    RelatedInput::Note(identifier) => {
                        benchmark.set_field("source_kind", "note");
                        benchmark.set_field("source_identifier_chars", identifier.chars().count());
                        if config.benchmark.include_query {
                            benchmark.set_field("source_identifier", identifier);
                        }
                    }
                    RelatedInput::Text(text) => {
                        benchmark.set_field("source_kind", "text");
                        benchmark.set_field("source_text_chars", text.chars().count());
                        if config.benchmark.include_query {
                            benchmark.set_field("source_text", text);
                        }
                    }
                }
            },
            |benchmark| {
                related::find_related_with_vector_cache(
                    &config,
                    input.clone(),
                    RelatedOptions {
                        limit: top,
                        candidates,
                    },
                    benchmark,
                    Some(vector_cache),
                )
            },
        )?;
        serde_json::to_value(report).map_err(Into::into)
    }

    pub(crate) fn tool_show(&self, arguments: &Value) -> Result<Value> {
        let chunk_id = arguments
            .get("chunk_id")
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .context("argument `chunk_id` must be a string")
            })
            .transpose()?;
        let chunk_ids_argument_present = arguments.get("chunk_ids").is_some();
        let mut chunk_ids = Vec::new();
        if let Some(chunk_id) = chunk_id {
            chunk_ids.push(chunk_id);
        }
        chunk_ids.extend(string_list_arg(arguments, "chunk_ids")?);
        if chunk_ids.is_empty() {
            bail!("missing string argument `chunk_id` or non-empty array argument `chunk_ids`");
        }

        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        if chunk_ids.len() == 1 && !chunk_ids_argument_present {
            let chunk_id = &chunk_ids[0];
            let chunk = db
                .load_chunk(chunk_id)?
                .with_context(|| format!("chunk not found: {chunk_id}"))?;
            serde_json::to_value(chunk).map_err(Into::into)
        } else {
            let report = db.load_chunks_by_id(&chunk_ids)?;
            serde_json::to_value(report).map_err(Into::into)
        }
    }

    pub(crate) fn tool_graph(&self, arguments: &Value) -> Result<Value> {
        let note = string_arg(arguments, "note")?;
        let depth = usize_arg(arguments, "depth", 1)?;
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        let view = db
            .graph_view(note, depth)?
            .with_context(|| format!("note not found: {note}"))?;
        serde_json::to_value(view).map_err(Into::into)
    }

    pub(crate) fn tool_tags(&self, arguments: &Value) -> Result<Value> {
        let prefix = optional_string_arg(arguments, "prefix");
        let top = usize_arg(arguments, "top", 50)?;
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        serde_json::to_value(db.tag_facets(prefix, top)?).map_err(Into::into)
    }

    pub(crate) fn tool_properties(&self, arguments: &Value) -> Result<Value> {
        let key = optional_string_arg(arguments, "key");
        let top = usize_arg(arguments, "top", 50)?;
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        serde_json::to_value(db.property_facets(key, top)?).map_err(Into::into)
    }

    pub(crate) fn tool_stats(&self) -> Result<Value> {
        let paths = KbPaths::from_config(&self.config);
        let db = Db::open(&paths.db_path)?;
        serde_json::to_value(db.stats()?).map_err(Into::into)
    }

    pub(crate) fn tool_warmup(&mut self) -> Result<Value> {
        self.warm_up_vector_cache()?;
        Ok(json!({
            "vector_embedder_loaded": self.vector_cache.is_loaded(),
            "vector_embeddings_loaded": self.vector_cache.embeddings_loaded(),
            "vector_embedding_count": self.vector_cache.cached_embedding_count()
        }))
    }

    pub(crate) fn tool_status(&self) -> Value {
        json!({
            "vector_embedder_loaded": self.vector_embedder_loaded(),
            "vector_embeddings_loaded": self.vector_cache.embeddings_loaded(),
            "vector_embedding_count": self.vector_cache.cached_embedding_count(),
            "idle_unload_seconds": self.config.mcp.idle_unload_seconds
        })
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
            "description": "Search the indexed Obsidian vault and return note-level results with matched chunks. Hybrid and vector searches reuse a warm local embedding model and cached stored embeddings while loaded.",
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
                        "description": "Tags to require; repeat values in the array for AND filters.",
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "tag": {
                        "description": "Alias for tags when passing a single tag.",
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string" } }
                        ]
                    },
                    "properties": {
                        "description": "Frontmatter property filters to require; repeat values in the array for AND filters.",
                        "type": "array",
                        "items": { "type": "string", "description": "KEY=VALUE, KEY!=VALUE, KEY>=VALUE, KEY<=VALUE, KEY>VALUE, or KEY<VALUE" }
                    },
                    "property": {
                        "description": "Alias for properties when passing a single property filter.",
                        "oneOf": [
                            { "type": "string" },
                            { "type": "array", "items": { "type": "string", "description": "KEY=VALUE, KEY!=VALUE, KEY>=VALUE, KEY<=VALUE, KEY>VALUE, or KEY<VALUE" } }
                        ]
                    }
                }
            }
        },
        {
            "name": "related",
            "description": "Find notes semantically related to an indexed note or draft text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note": {
                        "type": "string",
                        "description": "Indexed note identifier, path, title, or alias."
                    },
                    "text": {
                        "type": "string",
                        "description": "Draft text that is not indexed yet."
                    },
                    "top": { "type": "integer", "minimum": 0 },
                    "candidates": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Vector chunk candidates to score before note aggregation; 0 uses the configured default."
                    }
                }
            }
        },
        {
            "name": "show",
            "description": "Load one or more indexed chunks by chunk id. Pass either chunk_id or chunk_ids.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "chunk_id": {
                        "type": "string",
                        "description": "Single indexed chunk id. Preserves the legacy single-chunk response shape."
                    },
                    "chunk_ids": {
                        "type": "array",
                        "minItems": 1,
                        "items": { "type": "string" },
                        "description": "Indexed chunk ids to load in one batch. Returns an object with chunks and missing ids."
                    }
                }
            }
        },
        {
            "name": "graph",
            "description": "Return direct graph context around one indexed note.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "note": { "type": "string" },
                    "depth": { "type": "integer", "minimum": 0 }
                },
                "required": ["note"]
            }
        },
        {
            "name": "tags",
            "description": "List indexed tags available for search filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prefix": { "type": "string" },
                    "top": { "type": "integer", "minimum": 0 }
                }
            }
        },
        {
            "name": "properties",
            "description": "List indexed frontmatter property keys, or values for one key.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": { "type": "string" },
                    "top": { "type": "integer", "minimum": 0 }
                }
            }
        },
        {
            "name": "stats",
            "description": "Return index statistics.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "warmup",
            "description": "Initialize and keep the local embedding model and stored embeddings warm for subsequent vector searches.",
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

fn string_list_arg(arguments: &Value, name: &str) -> Result<Vec<String>> {
    let Some(value) = arguments.get(name) else {
        return Ok(Vec::new());
    };
    match value {
        Value::String(value) => Ok(vec![value.to_owned()]),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .with_context(|| format!("argument `{name}` must contain only strings"))
            })
            .collect(),
        _ => bail!("argument `{name}` must be a string or array of strings"),
    }
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
    use tempfile::TempDir;

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

    #[test]
    fn lists_metadata_tools_and_filter_arguments() {
        let tools = tools();

        assert!(has_tool(&tools, "search"));
        assert!(has_tool(&tools, "related"));
        assert!(has_tool(&tools, "tags"));
        assert!(has_tool(&tools, "properties"));

        let search = tool_named(&tools, "search");
        let search_args = &search["inputSchema"]["properties"];
        assert!(search_args.get("tags").is_some());
        assert!(search_args.get("tag").is_some());
        assert!(search_args.get("properties").is_some());
        assert!(search_args.get("property").is_some());

        let related = tool_named(&tools, "related");
        let related_args = &related["inputSchema"]["properties"];
        assert!(related_args.get("note").is_some());
        assert!(related_args.get("text").is_some());
        assert!(related_args.get("candidates").is_some());

        let show = tool_named(&tools, "show");
        let show_args = &show["inputSchema"]["properties"];
        assert!(show_args.get("chunk_id").is_some());
        assert!(show_args.get("chunk_ids").is_some());
    }

    #[test]
    fn tool_input_schemas_are_object_roots_without_combinators() {
        let tools = tools();
        let forbidden_root_keywords = ["oneOf", "anyOf", "allOf", "enum", "not"];

        for tool in tools.as_array().unwrap() {
            let schema = &tool["inputSchema"];
            assert_eq!(
                schema["type"], "object",
                "{} inputSchema must have an object root",
                tool["name"]
            );
            assert!(
                schema["properties"].is_object(),
                "{} inputSchema must declare object properties",
                tool["name"]
            );
            for keyword in forbidden_root_keywords {
                assert!(
                    schema.get(keyword).is_none(),
                    "{} inputSchema must not use {keyword} at the root",
                    tool["name"]
                );
            }
        }
    }

    #[test]
    fn metadata_tools_and_search_filters_work() {
        let (_temp, config) = indexed_test_config();
        let mut server = McpServer::new(config);

        let tags = call_tool_json(&mut server, "tags", json!({ "prefix": "research" }));
        assert_eq!(tags[0]["tag"], "research");

        let property_values = call_tool_json(&mut server, "properties", json!({ "key": "status" }));
        assert!(
            property_values["values"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value["value"] == "active")
        );

        let hits = call_tool_json(
            &mut server,
            "search",
            json!({
                "mode": "bm25",
                "tag": "research",
                "property": "status=active"
            }),
        );
        assert_eq!(hits[0]["path"], "alpha.md");

        let hits = call_tool_json(
            &mut server,
            "search",
            json!({
                "mode": "bm25",
                "tags": ["business"],
                "properties": ["status=done"]
            }),
        );
        assert_eq!(hits[0]["path"], "beta.md");
    }

    #[test]
    fn related_tool_returns_similar_notes() {
        let (_temp, config) = indexed_test_config();
        let paths = KbPaths::from_config(&config);
        let db = Db::open(&paths.db_path).unwrap();
        seed_test_embeddings(&db);
        let mut config = config;
        config.embeddings.enabled = true;
        let mut server = McpServer::new(config);

        let report = call_tool_json(
            &mut server,
            "related",
            json!({
                "note": "alpha.md",
                "top": 1
            }),
        );

        assert_eq!(report["source"]["kind"], "note");
        assert_eq!(report["source"]["path"], "alpha.md");
        assert_eq!(report["notes"][0]["path"], "beta.md");
    }

    #[test]
    fn show_tool_accepts_chunk_ids_batch() {
        let (_temp, config) = indexed_test_config();
        let paths = KbPaths::from_config(&config);
        let db = Db::open(&paths.db_path).unwrap();
        let chunks = db.load_all_chunks().unwrap();
        let chunk_ids = chunks
            .iter()
            .take(2)
            .map(|chunk| chunk.chunk_id.clone())
            .collect::<Vec<_>>();
        let mut server = McpServer::new(config);

        let report = call_tool_json(
            &mut server,
            "show",
            json!({
                "chunk_ids": [&chunk_ids[0], &chunk_ids[1], "missing-chunk"]
            }),
        );

        assert_eq!(report["chunks"].as_array().unwrap().len(), 2);
        assert_eq!(report["chunks"][0]["chunk_id"], chunk_ids[0]);
        assert_eq!(report["chunks"][1]["chunk_id"], chunk_ids[1]);
        assert_eq!(report["missing"][0], "missing-chunk");
    }

    #[test]
    fn related_tool_reuses_cached_embeddings_between_calls() {
        let (_temp, config) = indexed_test_config();
        let paths = KbPaths::from_config(&config);
        let db = Db::open(&paths.db_path).unwrap();
        seed_test_embeddings(&db);
        let mut config = config;
        config.embeddings.enabled = true;
        config.benchmark.enabled = true;
        let log_path = config.benchmark_log_path();
        let mut server = McpServer::new(config);

        for _ in 0..2 {
            let report = call_tool_json(
                &mut server,
                "related",
                json!({
                    "note": "alpha.md",
                    "top": 1
                }),
            );
            assert_eq!(report["notes"][0]["path"], "beta.md");
        }

        let content = std::fs::read_to_string(log_path).unwrap();
        let records = content
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["vector_embeddings_cached"], false);
        assert_eq!(records[1]["vector_embeddings_cached"], true);
        assert_eq!(records[1]["phases"]["vector_load_embeddings_ms"], 0.0);
    }

    fn has_tool(tools: &Value, name: &str) -> bool {
        tools
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == name)
    }

    fn tool_named<'a>(tools: &'a Value, name: &str) -> &'a Value {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap()
    }

    fn call_tool_json(server: &mut McpServer, name: &str, arguments: Value) -> Value {
        let result = server
            .call_tool(&json!({ "name": name, "arguments": arguments }))
            .unwrap();
        assert_eq!(result["isError"], false);
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn indexed_test_config() -> (TempDir, AppConfig) {
        let temp = tempfile::tempdir().unwrap();
        let vault = temp.path().join("vault");
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(
            vault.join("alpha.md"),
            r#"---
tags: [research, ai/context]
status: active
priority: 2
---
# Alpha

Alpha retrieval note.
"#,
        )
        .unwrap();
        std::fs::write(
            vault.join("beta.md"),
            r#"---
tags: [business]
status: done
priority: 1
---
# Beta

Beta planning note.
"#,
        )
        .unwrap();

        let mut config = AppConfig::default_for_vault_in(&vault, None, temp.path()).unwrap();
        config.embeddings.enabled = false;
        config.search.default_mode = "bm25".to_string();

        let mut notes = crate::vault::load_vault(&config).unwrap();
        let graph_report = crate::graph::resolve_links(&mut notes);
        let paths = KbPaths::from_config(&config);
        let mut db = Db::open(&paths.db_path).unwrap();
        db.replace_index(&notes, &graph_report.warnings).unwrap();
        let chunks = db.load_all_chunks().unwrap();
        crate::tantivy_index::rebuild(&paths.tantivy_dir, &chunks, config.index.remove_diacritics)
            .unwrap();

        (temp, config)
    }

    fn seed_test_embeddings(db: &Db) {
        let chunks = db.load_all_chunks().unwrap();
        for chunk in chunks {
            let vector = if chunk.note_path == "alpha.md" {
                [1.0, 0.0, 0.0]
            } else {
                [0.97, 0.03, 0.0]
            };
            db.insert_embedding(
                &chunk.chunk_id,
                "fastembed:MultilingualE5Small",
                3,
                &crate::embeddings::encode_vector(&vector),
                &chunk.text_hash,
            )
            .unwrap();
        }
    }
}
