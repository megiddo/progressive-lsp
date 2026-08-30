//! `LspClient` Facade, `LspLocation`, `SpawnSpec`, and `StdioLsp` Adapter.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::str::FromStr;

use lsp_types::{
    ClientCapabilities, GotoDefinitionResponse, InitializeParams, Location, LocationLink, Uri,
};
use serde_json::{json, Value};

use crate::buffer::{BufferMap, Selection};
use crate::error::IdeError;
use crate::language::{LanguageCatalog, ServeMode};
use crate::ports::{FsPort, LspTransport};
use crate::tabs::{TabId, TabStrip};

const MAX_HEADER_LINE: usize = 4096;
const METHOD_NOT_FOUND: i64 = -32601;
const METHOD_INITIALIZE: &str = "initialize";
const METHOD_INITIALIZED: &str = "initialized";
const METHOD_SHUTDOWN: &str = "shutdown";
const METHOD_EXIT: &str = "exit";
const METHOD_DID_OPEN: &str = "textDocument/didOpen";
const METHOD_DID_CHANGE: &str = "textDocument/didChange";
const METHOD_DID_SAVE: &str = "textDocument/didSave";
const METHOD_DID_CLOSE: &str = "textDocument/didClose";
const METHOD_DEFINITION: &str = "textDocument/definition";
const METHOD_IMPLEMENTATION: &str = "textDocument/implementation";
const METHOD_REFERENCES: &str = "textDocument/references";

/// Advertised `experimental.progressiveLsp`. Socket may be null. `LspClient`
/// never opens it; [`crate::control::ControlClient`] does when
/// [`crate::language::ServeMode::ControlSocket`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressiveLspCap {
    version: String,
    socket: Option<String>,
    mux: bool,
}

impl ProgressiveLspCap {
    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn socket(&self) -> Option<&str> {
        self.socket.as_deref()
    }

    pub fn mux(&self) -> bool {
        self.mux
    }

    pub fn from_initialize_result(value: &Value) -> Option<Self> {
        let cap = value
            .get("capabilities")
            .and_then(|c| c.get("experimental"))
            .and_then(|e| e.get("progressiveLsp"))?;
        if !cap.is_object() {
            return None;
        }
        let version = cap
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("v1")
            .to_string();
        let socket = match cap.get("socket") {
            None | Some(Value::Null) => None,
            Some(v) => v.as_str().map(str::to_string),
        };
        let mux = cap.get("mux").and_then(|m| m.as_bool()).unwrap_or(false);
        Some(Self {
            version,
            socket,
            mux,
        })
    }
}

/// uri + range from the client. Jump opens or focuses a tab; empty list is valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LspLocation {
    uri: String,
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
}

impl LspLocation {
    pub fn new(
        uri: impl Into<String>,
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
    ) -> Self {
        Self {
            uri: uri.into(),
            start_line,
            start_character,
            end_line,
            end_character,
        }
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn start_line(&self) -> u32 {
        self.start_line
    }

    pub fn start_character(&self) -> u32 {
        self.start_character
    }

    pub fn end_line(&self) -> u32 {
        self.end_line
    }

    pub fn end_character(&self) -> u32 {
        self.end_character
    }

    pub fn file_path(&self) -> Result<PathBuf, IdeError> {
        path_from_file_uri(&self.uri)
    }

    pub fn to_selection(&self, text: &str) -> Selection {
        let start = offset_at(text, self.start_line, self.start_character);
        let end = offset_at(text, self.end_line, self.end_character);
        Selection::new(start, end)
    }

    pub fn open_or_focus(
        &self,
        tabs: &mut TabStrip,
        buffers: &mut BufferMap,
        fs: &impl FsPort,
    ) -> Result<(), IdeError> {
        let path = self.file_path()?;
        let buf = buffers.open(&path, fs)?;
        let sel = self.to_selection(&buf.text());
        buf.set_selection(sel);
        tabs.open(&path);
        Ok(())
    }
}

/// Binary from env, then `target/{debug,release}/progressive-lsp`, then `PATH`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    binary: PathBuf,
}

impl SpawnSpec {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            binary: path.into(),
        }
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn resolve() -> Result<Self, IdeError> {
        Self::resolve_in(
            std::env::var_os("PROGRESSIVE_LSP"),
            std::env::current_dir().ok().as_deref(),
            std::env::var_os("PATH"),
        )
    }

    pub fn resolve_in(
        env_binary: Option<OsString>,
        cwd: Option<&Path>,
        path_env: Option<OsString>,
    ) -> Result<Self, IdeError> {
        if let Some(p) = env_binary {
            if !p.is_empty() {
                let path = PathBuf::from(p);
                if path.is_file() {
                    return Ok(Self { binary: path });
                }
                return Err(IdeError::MissingBinary);
            }
        }
        if let Some(cwd) = cwd {
            for rel in [
                "target/debug/progressive-lsp",
                "target/release/progressive-lsp",
            ] {
                let candidate = cwd.join(rel);
                if candidate.is_file() {
                    return Ok(Self { binary: candidate });
                }
            }
        }
        if let Some(path_env) = path_env {
            for dir in std::env::split_paths(&path_env) {
                let candidate = dir.join("progressive-lsp");
                if candidate.is_file() {
                    return Ok(Self { binary: candidate });
                }
            }
        }
        Err(IdeError::MissingBinary)
    }
}

/// Content-Length JSON-RPC over child stdio (or a test pair).
pub struct StdioLsp {
    child: Option<Child>,
    writer: Box<dyn Write + Send>,
    reader: Box<dyn BufRead + Send>,
    next_id: i64,
}

impl StdioLsp {
    pub fn spawn(spec: &SpawnSpec) -> Result<Self, IdeError> {
        Self::spawn_serve(spec, ServeMode::StockStdio, None)
    }

    pub fn spawn_serve(
        spec: &SpawnSpec,
        mode: ServeMode,
        control_socket: Option<&Path>,
    ) -> Result<Self, IdeError> {
        let args = mode.serve_args(control_socket)?;
        Self::spawn_with_args(spec, &args)
    }

    pub fn spawn_with_args(spec: &SpawnSpec, args: &[impl AsRef<OsStr>]) -> Result<Self, IdeError> {
        let mut cmd = Command::new(spec.binary());
        for arg in args {
            cmd.arg(arg);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    IdeError::MissingBinary
                } else {
                    IdeError::lsp(e.to_string())
                }
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| IdeError::lsp("child stdin missing"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| IdeError::lsp("child stdout missing"))?;
        Ok(Self {
            child: Some(child),
            writer: Box::new(stdin),
            reader: Box::new(std::io::BufReader::new(stdout)),
            next_id: 1,
        })
    }

    pub fn from_pair(
        writer: impl Write + Send + 'static,
        reader: impl BufRead + Send + 'static,
    ) -> Self {
        Self {
            child: None,
            writer: Box::new(writer),
            reader: Box::new(reader),
            next_id: 1,
        }
    }

    pub fn next_id(&self) -> i64 {
        self.next_id
    }
}

impl std::fmt::Debug for StdioLsp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StdioLsp")
            .field("has_child", &self.child.is_some())
            .field("next_id", &self.next_id)
            .finish()
    }
}

impl Drop for StdioLsp {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl LspTransport for StdioLsp {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, IdeError> {
        let id = self.next_id;
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, &to_vec(&msg)?)?;
        loop {
            let body = read_message(&mut self.reader)?
                .ok_or_else(|| IdeError::lsp("eof waiting for response"))?;
            let v: Value =
                serde_json::from_slice(&body).map_err(|e| IdeError::lsp(e.to_string()))?;
            if v.get("id").is_none() {
                continue;
            }
            if v.get("id") != Some(&json!(id)) {
                continue;
            }
            if let Some(err) = v.get("error") {
                let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
                if code == METHOD_NOT_FOUND {
                    return Err(IdeError::lsp_method_missing(method));
                }
                return Err(IdeError::lsp(err.to_string()));
            }
            return Ok(v.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), IdeError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.writer, &to_vec(&msg)?)
    }
}

/// JSON-RPC in; domain locations out. No watch internals.
pub struct LspClient<T: LspTransport> {
    transport: T,
    mode: ServeMode,
    catalog: LanguageCatalog,
    ready: bool,
    cap: Option<ProgressiveLspCap>,
    versions: BTreeMap<PathBuf, i32>,
}

impl<T: LspTransport> LspClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            mode: ServeMode::StockStdio,
            catalog: LanguageCatalog::new(),
            ready: false,
            cap: None,
            versions: BTreeMap::new(),
        }
    }

    pub fn with_mode(mut self, mode: ServeMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_catalog(mut self, catalog: LanguageCatalog) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn serve_mode(&self) -> ServeMode {
        self.mode
    }

    pub fn catalog(&self) -> &LanguageCatalog {
        &self.catalog
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn progressive_cap(&self) -> Option<&ProgressiveLspCap> {
        self.cap.as_ref()
    }

    pub fn open_version(&self, path: impl AsRef<Path>) -> Option<i32> {
        self.versions.get(path.as_ref()).copied()
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn into_inner(self) -> T {
        self.transport
    }

    pub fn initialize(&mut self, root: impl AsRef<Path>) -> Result<(), IdeError> {
        if self.ready {
            return Err(IdeError::lsp("already initialized"));
        }
        let uri = file_uri(root.as_ref())?;
        #[allow(deprecated)]
        let typed = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(Uri::from_str(&uri).map_err(|e| IdeError::lsp(e.to_string()))?),
            capabilities: ClientCapabilities::default(),
            ..Default::default()
        };
        let params = serde_json::to_value(typed).map_err(|e| IdeError::lsp(e.to_string()))?;
        let result = self.transport.request(METHOD_INITIALIZE, params)?;
        self.cap = ProgressiveLspCap::from_initialize_result(&result);
        self.transport.notify(METHOD_INITIALIZED, json!({}))?;
        self.ready = true;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), IdeError> {
        self.require_ready()?;
        self.transport.request(METHOD_SHUTDOWN, Value::Null)?;
        self.transport.notify(METHOD_EXIT, Value::Null)?;
        self.ready = false;
        self.versions.clear();
        Ok(())
    }

    pub fn did_open(&mut self, path: impl AsRef<Path>, text: &str) -> Result<bool, IdeError> {
        self.require_ready()?;
        let path = path.as_ref();
        if self.catalog.skips_did_open(path) {
            return Ok(false);
        }
        let language_id = self.catalog.for_path(path).to_string();
        let uri = file_uri(path)?;
        let params = json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text
            }
        });
        self.transport.notify(METHOD_DID_OPEN, params)?;
        self.versions.insert(path.to_path_buf(), 1);
        Ok(true)
    }

    pub fn did_change(
        &mut self,
        path: impl AsRef<Path>,
        old_text: &str,
        new_text: &str,
    ) -> Result<(), IdeError> {
        self.require_ready()?;
        let path = path.as_ref();
        let Some(version) = self.versions.get_mut(path) else {
            return Ok(());
        };
        if old_text == new_text {
            return Ok(());
        }
        *version = version.saturating_add(1);
        let version = *version;
        let change = incremental_edit(old_text, new_text);
        let uri = file_uri(path)?;
        let params = json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{
                "range": {
                    "start": { "line": change.0, "character": change.1 },
                    "end": { "line": change.2, "character": change.3 }
                },
                "text": change.4
            }]
        });
        self.transport.notify(METHOD_DID_CHANGE, params)
    }

    pub fn did_save(&mut self, path: impl AsRef<Path>) -> Result<(), IdeError> {
        self.require_ready()?;
        let path = path.as_ref();
        if !self.versions.contains_key(path) {
            return Ok(());
        }
        let uri = file_uri(path)?;
        self.transport
            .notify(METHOD_DID_SAVE, json!({ "textDocument": { "uri": uri } }))
    }

    pub fn did_close(&mut self, path: impl AsRef<Path>) -> Result<(), IdeError> {
        self.require_ready()?;
        let path = path.as_ref();
        if self.versions.remove(path).is_none() {
            return Ok(());
        }
        let uri = file_uri(path)?;
        self.transport
            .notify(METHOD_DID_CLOSE, json!({ "textDocument": { "uri": uri } }))
    }

    pub fn definition(
        &mut self,
        path: impl AsRef<Path>,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>, IdeError> {
        self.discover(METHOD_DEFINITION, path.as_ref(), line, character, false)
    }

    pub fn implementation(
        &mut self,
        path: impl AsRef<Path>,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>, IdeError> {
        self.discover(METHOD_IMPLEMENTATION, path.as_ref(), line, character, false)
    }

    pub fn references(
        &mut self,
        path: impl AsRef<Path>,
        line: u32,
        character: u32,
    ) -> Result<Vec<LspLocation>, IdeError> {
        self.discover(METHOD_REFERENCES, path.as_ref(), line, character, true)
    }

    /// Open or focus a tab at each range. Empty list is valid (T3 stub / method missing).
    pub fn jump(
        locations: &[LspLocation],
        tabs: &mut TabStrip,
        buffers: &mut BufferMap,
        fs: &impl FsPort,
    ) -> Result<usize, IdeError> {
        if locations.is_empty() {
            return Ok(0);
        }
        for loc in locations {
            loc.open_or_focus(tabs, buffers, fs)?;
        }
        let first = locations[0].file_path()?;
        tabs.focus(&TabId::from_path(first));
        Ok(locations.len())
    }

    fn discover(
        &mut self,
        method: &str,
        path: &Path,
        line: u32,
        character: u32,
        references: bool,
    ) -> Result<Vec<LspLocation>, IdeError> {
        self.require_ready()?;
        let uri = file_uri(path)?;
        let mut params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });
        if references {
            params["context"] = json!({ "includeDeclaration": true });
        }
        match self.transport.request(method, params) {
            Ok(v) => parse_locations(&v),
            Err(e) if e.is_lsp_method_missing() => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    fn require_ready(&self) -> Result<(), IdeError> {
        if self.ready {
            Ok(())
        } else {
            Err(IdeError::lsp("not initialized"))
        }
    }
}

/// UTF-16 LSP position for a char offset.
pub fn position_at(text: &str, char_offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in text.chars().enumerate() {
        if i == char_offset {
            return (line, col);
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    (line, col)
}

pub fn file_uri(path: &Path) -> Result<String, IdeError> {
    if !path.is_absolute() {
        return Err(IdeError::NotAbsolute(path.to_path_buf()));
    }
    Ok(format!("file://{}", path.to_string_lossy()))
}

pub fn path_from_file_uri(uri: &str) -> Result<PathBuf, IdeError> {
    let rest = uri
        .strip_prefix("file://")
        .ok_or_else(|| IdeError::lsp(format!("not a file uri: {uri}")))?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    if decoded.starts_with('/') {
        Ok(PathBuf::from(decoded))
    } else {
        Ok(PathBuf::from(format!("/{decoded}")))
    }
}

pub fn encode_message(body: impl AsRef<[u8]>) -> Vec<u8> {
    let body = body.as_ref();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut out = Vec::with_capacity(header.len() + body.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(body);
    out
}

pub fn write_message<W: Write>(writer: &mut W, body: impl AsRef<[u8]>) -> Result<(), IdeError> {
    writer
        .write_all(&encode_message(body))
        .map_err(|e| IdeError::lsp(e.to_string()))?;
    writer.flush().map_err(|e| IdeError::lsp(e.to_string()))
}

pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<Vec<u8>>, IdeError> {
    let mut content_length: Option<usize> = None;
    let mut saw_any = false;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| IdeError::lsp(e.to_string()))?;
        if n == 0 {
            return if saw_any {
                Err(IdeError::lsp("eof mid-headers"))
            } else {
                Ok(None)
            };
        }
        if line.len() > MAX_HEADER_LINE {
            return Err(IdeError::lsp("header line too long"));
        }
        saw_any = true;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                let parsed = value.trim().parse::<usize>().map_err(|_| {
                    IdeError::lsp(format!("invalid Content-Length: {}", value.trim()))
                })?;
                content_length = Some(parsed);
            }
        }
    }
    let len = content_length.ok_or_else(|| IdeError::lsp("missing Content-Length"))?;
    let mut buf = vec![0u8; len];
    reader
        .read_exact(&mut buf)
        .map_err(|e| IdeError::lsp(e.to_string()))?;
    Ok(Some(buf))
}

fn offset_at(text: &str, line: u32, character: u32) -> usize {
    let mut chars = 0usize;
    let mut cur_line = 0u32;
    let mut cur_col = 0u32;
    for ch in text.chars() {
        if cur_line == line && cur_col >= character {
            return chars;
        }
        if ch == '\n' {
            if cur_line == line {
                return chars;
            }
            cur_line += 1;
            cur_col = 0;
            chars += 1;
            continue;
        }
        if cur_line == line {
            cur_col += ch.len_utf16() as u32;
        }
        chars += 1;
    }
    chars
}

fn incremental_edit(old: &str, new: &str) -> (u32, u32, u32, u32, String) {
    let old_chars: Vec<char> = old.chars().collect();
    let new_chars: Vec<char> = new.chars().collect();
    let mut prefix = 0;
    while prefix < old_chars.len()
        && prefix < new_chars.len()
        && old_chars[prefix] == new_chars[prefix]
    {
        prefix += 1;
    }
    let mut old_end = old_chars.len();
    let mut new_end = new_chars.len();
    while old_end > prefix && new_end > prefix && old_chars[old_end - 1] == new_chars[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    let (sl, sc) = position_at(old, prefix);
    let (el, ec) = position_at(old, old_end);
    let mid: String = new_chars[prefix..new_end].iter().collect();
    (sl, sc, el, ec, mid)
}

fn parse_locations(value: &Value) -> Result<Vec<LspLocation>, IdeError> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Ok(resp) = serde_json::from_value::<GotoDefinitionResponse>(value.clone()) {
        return Ok(locations_from_goto(resp));
    }
    if let Ok(locs) = serde_json::from_value::<Vec<Location>>(value.clone()) {
        return Ok(locs.into_iter().map(from_location).collect());
    }
    Err(IdeError::lsp("invalid location result"))
}

fn locations_from_goto(resp: GotoDefinitionResponse) -> Vec<LspLocation> {
    match resp {
        GotoDefinitionResponse::Scalar(loc) => vec![from_location(loc)],
        GotoDefinitionResponse::Array(locs) => locs.into_iter().map(from_location).collect(),
        GotoDefinitionResponse::Link(links) => links.into_iter().map(from_link).collect(),
    }
}

fn from_location(loc: Location) -> LspLocation {
    LspLocation::new(
        loc.uri.as_str().to_string(),
        loc.range.start.line,
        loc.range.start.character,
        loc.range.end.line,
        loc.range.end.character,
    )
}

fn from_link(link: LocationLink) -> LspLocation {
    let range = link.target_selection_range;
    LspLocation::new(
        link.target_uri.as_str().to_string(),
        range.start.line,
        range.start.character,
        range.end.line,
        range.end.character,
    )
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(h) = hex_byte(bytes[i + 1], bytes[i + 2]) {
                out.push(h);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_byte(a: u8, b: u8) -> Option<u8> {
    Some((hex_val(a)? << 4) | hex_val(b)?)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn to_vec(value: &Value) -> Result<Vec<u8>, IdeError> {
    serde_json::to_vec(value).map_err(|e| IdeError::lsp(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeLsp, MemFs};
    use std::fs;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    fn location_json(uri: &str, sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({
            "uri": uri,
            "range": {
                "start": { "line": sl, "character": sc },
                "end": { "line": el, "character": ec }
            }
        })
    }

    fn init_result(socket: Option<&str>) -> Value {
        json!({
            "capabilities": {
                "experimental": {
                    "progressiveLsp": {
                        "version": "v1",
                        "socket": socket,
                        "mux": false
                    }
                }
            }
        })
    }

    fn ready_client(fake: FakeLsp) -> LspClient<FakeLsp> {
        let mut client = LspClient::new(fake);
        client.initialize("/ws").unwrap();
        client
    }

    struct CaptureWrite(Arc<Mutex<Vec<u8>>>);

    impl Write for CaptureWrite {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("write lock").write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn scripted_init(socket: Option<&str>) -> FakeLsp {
        let mut fake = FakeLsp::new();
        fake.script(METHOD_INITIALIZE, init_result(socket));
        fake
    }

    #[test]
    fn progressive_lsp_cap_socket_may_be_null() {
        let cap = ProgressiveLspCap::from_initialize_result(&init_result(None)).unwrap();
        assert_eq!(cap.version(), "v1");
        assert!(cap.socket().is_none());
        assert!(!cap.mux());
        let with_sock =
            ProgressiveLspCap::from_initialize_result(&init_result(Some("/tmp/plsp.sock")))
                .unwrap();
        assert_eq!(with_sock.socket(), Some("/tmp/plsp.sock"));
        assert!(ProgressiveLspCap::from_initialize_result(&json!({})).is_none());
        assert!(ProgressiveLspCap::from_initialize_result(&json!({
            "capabilities": { "experimental": { "progressiveLsp": null } }
        }))
        .is_none());
        assert!(ProgressiveLspCap::from_initialize_result(&json!({
            "capabilities": { "experimental": { "progressiveLsp": "nope" } }
        }))
        .is_none());
        let missing_ver = ProgressiveLspCap::from_initialize_result(&json!({
            "capabilities": { "experimental": { "progressiveLsp": { "mux": true } } }
        }))
        .unwrap();
        assert_eq!(missing_ver.version(), "v1");
        assert!(missing_ver.mux());
        assert!(missing_ver.socket().is_none());
    }

    #[test]
    fn spawn_spec_value_object_missing_is_domain_error() {
        let err =
            SpawnSpec::resolve_in(None, Some(Path::new("/no-such-cwd-ide4")), None).unwrap_err();
        assert!(err.is_missing_binary());
        assert_eq!(err.to_string(), "progressive-lsp binary not found");
        let missing_env = SpawnSpec::resolve_in(
            Some(OsString::from("/no/such/progressive-lsp-ide4")),
            None,
            None,
        )
        .unwrap_err();
        assert!(missing_env.is_missing_binary());
        let empty_env = SpawnSpec::resolve_in(Some(OsString::from("")), None, None).unwrap_err();
        assert!(empty_env.is_missing_binary());
        let spec = SpawnSpec::from_path("/opt/progressive-lsp");
        assert_eq!(spec.binary(), Path::new("/opt/progressive-lsp"));
        assert_eq!(spec, SpawnSpec::from_path("/opt/progressive-lsp"));
    }

    #[test]
    fn spawn_spec_value_object_env_then_target_then_path() {
        let tmp = tempfile::tempdir().unwrap();
        let env_bin = tmp.path().join("from-env");
        let debug_bin = tmp.path().join("target/debug/progressive-lsp");
        let release_bin = tmp.path().join("target/release/progressive-lsp");
        let path_dir = tmp.path().join("bin");
        let path_bin = path_dir.join("progressive-lsp");
        fs::create_dir_all(debug_bin.parent().unwrap()).unwrap();
        fs::create_dir_all(release_bin.parent().unwrap()).unwrap();
        fs::create_dir_all(&path_dir).unwrap();
        fs::write(&env_bin, b"e").unwrap();
        fs::write(&debug_bin, b"d").unwrap();
        fs::write(&release_bin, b"r").unwrap();
        fs::write(&path_bin, b"p").unwrap();

        let via_env = SpawnSpec::resolve_in(
            Some(env_bin.clone().into_os_string()),
            Some(tmp.path()),
            Some(path_dir.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(via_env.binary(), env_bin.as_path());

        let via_debug = SpawnSpec::resolve_in(
            None,
            Some(tmp.path()),
            Some(path_dir.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(via_debug.binary(), debug_bin.as_path());

        fs::remove_file(&debug_bin).unwrap();
        let via_release = SpawnSpec::resolve_in(
            None,
            Some(tmp.path()),
            Some(path_dir.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(via_release.binary(), release_bin.as_path());

        fs::remove_file(&release_bin).unwrap();
        fs::create_dir_all(&debug_bin).unwrap();
        let via_path = SpawnSpec::resolve_in(
            None,
            Some(tmp.path()),
            Some(path_dir.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(via_path.binary(), path_bin.as_path());

        let env_dir = SpawnSpec::resolve_in(
            Some(tmp.path().as_os_str().to_os_string()),
            Some(tmp.path()),
            Some(path_dir.into_os_string()),
        )
        .unwrap_err();
        assert!(env_dir.is_missing_binary());
    }

    #[test]
    fn stdio_lsp_adapter_content_length_round_trip() {
        let framed = encode_message(b"hi");
        assert_eq!(&framed, b"Content-Length: 2\r\n\r\nhi");
        let mut out = Vec::new();
        write_message(&mut out, b"z").unwrap();
        assert_eq!(out, encode_message(b"z"));

        let mut raw = b"Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n".to_vec();
        raw.extend_from_slice(&encode_message(b"{\"a\":1}"));
        let mut cur = Cursor::new(raw);
        assert_eq!(read_message(&mut cur).unwrap().unwrap(), b"{\"a\":1}");
        assert!(read_message(&mut cur).unwrap().is_none());

        let mut case = Cursor::new(*b"content-length: 1\r\n\r\nZ");
        assert_eq!(read_message(&mut case).unwrap().unwrap(), b"Z");

        assert!(read_message(&mut Cursor::new(b"X: 1\r\n\r\n"))
            .unwrap_err()
            .to_string()
            .contains("missing Content-Length"));
        assert!(
            read_message(&mut Cursor::new(b"Content-Length: nope\r\n\r\n"))
                .unwrap_err()
                .to_string()
                .contains("invalid Content-Length")
        );
        let long = format!("X: {}\r\n", "a".repeat(MAX_HEADER_LINE));
        assert!(read_message(&mut Cursor::new(long))
            .unwrap_err()
            .to_string()
            .contains("header line too long"));
        let exact = format!("{}\n", "x".repeat(MAX_HEADER_LINE.saturating_sub(1)));
        let exact_err = read_message(&mut Cursor::new(exact)).unwrap_err();
        assert!(
            !exact_err.to_string().contains("header line too long"),
            "a line of exactly {MAX_HEADER_LINE} bytes is allowed: {exact_err}"
        );
        assert!(read_message(&mut Cursor::new(b"Content-Length: 1"))
            .unwrap_err()
            .to_string()
            .contains("eof mid-headers"));
    }

    #[test]
    fn stdio_lsp_adapter_request_skips_notifications() {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let mut bytes = encode_message(
            br#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"hi"}}"#,
        );
        bytes.extend_from_slice(&encode_message(
            br#"{"jsonrpc":"2.0","id":99,"result":null}"#,
        ));
        bytes.extend_from_slice(&encode_message(
            br#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        ));
        let mut lsp = StdioLsp::from_pair(CaptureWrite(writes.clone()), Cursor::new(bytes));
        let debug = format!("{lsp:?}");
        assert!(debug.contains("StdioLsp"));
        assert!(debug.contains("has_child: false"));
        assert!(debug.contains("next_id: 1"));
        assert_eq!(lsp.next_id(), 1);
        let result = lsp.request("initialize", json!({})).unwrap();
        assert_eq!(result, json!({"ok": true}));
        assert_eq!(lsp.next_id(), 2);
        let sent = writes.lock().unwrap().clone();
        assert!(String::from_utf8_lossy(&sent).contains("initialize"));
        lsp.notify("initialized", json!({})).unwrap();
        let sent = writes.lock().unwrap().clone();
        assert!(String::from_utf8_lossy(&sent).contains("initialized"));
    }

    #[test]
    fn stdio_lsp_adapter_method_missing_and_errors() {
        let err_body = encode_message(
            br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        );
        let mut lsp = StdioLsp::from_pair(Vec::new(), Cursor::new(err_body));
        assert!(lsp
            .request(METHOD_IMPLEMENTATION, json!({}))
            .unwrap_err()
            .is_lsp_method_missing());

        let other =
            encode_message(br#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"boom"}}"#);
        let mut lsp = StdioLsp::from_pair(Vec::new(), Cursor::new(other));
        let err = lsp.request("shutdown", json!(null)).unwrap_err();
        assert!(err.is_lsp());
        assert!(err.to_string().contains("boom"));

        let mut lsp = StdioLsp::from_pair(Vec::new(), Cursor::new(Vec::new()));
        assert!(lsp.request("initialize", json!({})).unwrap_err().is_lsp());

        let bad = encode_message(b"not-json");
        let mut lsp = StdioLsp::from_pair(Vec::new(), Cursor::new(bad));
        assert!(lsp.request("initialize", json!({})).unwrap_err().is_lsp());

        let no_result = encode_message(br#"{"jsonrpc":"2.0","id":1}"#);
        let mut lsp = StdioLsp::from_pair(Vec::new(), Cursor::new(no_result));
        assert_eq!(lsp.request("shutdown", Value::Null).unwrap(), Value::Null);
    }

    #[test]
    fn stdio_lsp_adapter_spawn_missing_binary() {
        let spec = SpawnSpec::from_path("/no/such/progressive-lsp-ide4-bin");
        assert!(StdioLsp::spawn(&spec).unwrap_err().is_missing_binary());
        assert!(StdioLsp::spawn_serve(&spec, ServeMode::StockStdio, None)
            .unwrap_err()
            .is_missing_binary());
        assert!(StdioLsp::spawn_serve(
            &spec,
            ServeMode::ControlSocket,
            Some(Path::new("/tmp/poc-ide5.sock")),
        )
        .unwrap_err()
        .is_missing_binary());
        assert!(StdioLsp::spawn_serve(&spec, ServeMode::ControlSocket, None)
            .unwrap_err()
            .is_control_socket_missing());
        let args = ServeMode::ControlSocket
            .serve_args(Some(Path::new("/tmp/poc-ide5.sock")))
            .unwrap();
        assert!(StdioLsp::spawn_with_args(&spec, &args)
            .unwrap_err()
            .is_missing_binary());
        assert!(!args.iter().any(|a| a == "--mux"));
    }

    #[cfg(unix)]
    #[test]
    fn stdio_lsp_adapter_spawn_immediate_exit_is_domain_result() {
        let spec = SpawnSpec::from_path("/usr/bin/true");
        let mut lsp = StdioLsp::spawn(&spec).unwrap();
        assert!(lsp.request("initialize", json!({})).unwrap_err().is_lsp());
    }

    #[test]
    fn lsp_client_facade_initialize_reads_experimental_cap() {
        let mut client = LspClient::new(scripted_init(None)).with_mode(ServeMode::ControlSocket);
        assert_eq!(client.serve_mode(), ServeMode::ControlSocket);
        assert!(!client.is_ready());
        client.initialize("/ws").unwrap();
        assert!(client.is_ready());
        let cap = client.progressive_cap().unwrap();
        assert_eq!(cap.version(), "v1");
        assert!(cap.socket().is_none());
        assert!(!cap.mux());
        assert!(client.initialize("/ws").unwrap_err().is_lsp());
        let inner = client.into_inner();
        assert_eq!(
            inner.sent_methods(),
            vec![METHOD_INITIALIZE, METHOD_INITIALIZED]
        );
        let init_params = &inner.sent()[0].params;
        assert!(
            init_params
                .get("processId")
                .and_then(|v| v.as_u64())
                .is_some(),
            "initialize must send processId: {init_params}"
        );
        assert_eq!(init_params["rootUri"], "file:///ws");
        assert!(
            init_params.get("capabilities").is_some(),
            "initialize must send capabilities: {init_params}"
        );
        assert!(!inner.sent()[0].is_notification());
        assert!(inner.sent()[1].is_notification());
        assert!(inner
            .sent()
            .iter()
            .all(|c| !c.method.contains("filesSince") && !c.method.starts_with("$/")));
    }

    #[test]
    fn lsp_client_facade_does_not_connect_control_socket() {
        let mut client = LspClient::new(scripted_init(Some("/tmp/plsp.sock")))
            .with_mode(ServeMode::ControlSocket);
        client.initialize("/ws").unwrap();
        assert_eq!(
            client.progressive_cap().unwrap().socket(),
            Some("/tmp/plsp.sock")
        );
        assert!(client.serve_mode().is_control_socket());
        assert!(client.catalog().skips_did_open("/ws/a.txt"));
    }

    #[test]
    fn lsp_client_facade_plaintext_skips_did_open() {
        let mut client = ready_client(scripted_init(None));
        assert!(!client.did_open("/ws/notes.txt", "hi").unwrap());
        assert!(client.open_version("/ws/notes.txt").is_none());
        assert!(client.did_open("/ws/lib.rs", "fn x() {}\n").unwrap());
        assert_eq!(client.open_version("/ws/lib.rs"), Some(1));
        client
            .did_change("/ws/lib.rs", "fn x() {}\n", "fn y() {}\n")
            .unwrap();
        assert_eq!(client.open_version("/ws/lib.rs"), Some(2));
        client
            .did_change("/ws/lib.rs", "fn y() {}\n", "fn y() {}\n")
            .unwrap();
        assert_eq!(client.open_version("/ws/lib.rs"), Some(2));
        client.did_change("/ws/notes.txt", "hi", "ho").unwrap();
        client.did_save("/ws/notes.txt").unwrap();
        client.did_close("/ws/notes.txt").unwrap();
        client.did_save("/ws/lib.rs").unwrap();
        client.did_close("/ws/lib.rs").unwrap();
        assert!(client.open_version("/ws/lib.rs").is_none());
        let inner = client.into_inner();
        assert_eq!(
            inner.sent_methods(),
            vec![
                METHOD_INITIALIZE,
                METHOD_INITIALIZED,
                METHOD_DID_OPEN,
                METHOD_DID_CHANGE,
                METHOD_DID_SAVE,
                METHOD_DID_CLOSE
            ]
        );
        assert_eq!(inner.sent()[2].params["textDocument"]["languageId"], "rust");
        assert_eq!(inner.sent()[3].params["textDocument"]["version"], 2);
        assert_eq!(
            inner.sent()[4].params["textDocument"]["uri"],
            "file:///ws/lib.rs"
        );
        assert!(inner.sent()[3].params["contentChanges"][0]
            .get("range")
            .is_some());
    }

    #[test]
    fn lsp_client_facade_definition_implementation_references() {
        let mut fake = scripted_init(None);
        fake.script(
            METHOD_DEFINITION,
            location_json("file:///ws/lib.rs", 0, 3, 0, 4),
        );
        fake.script(
            METHOD_IMPLEMENTATION,
            json!([
                location_json("file:///ws/a.rs", 1, 0, 1, 4),
                location_json("file:///ws/b.rs", 2, 0, 2, 1)
            ]),
        );
        fake.script(METHOD_REFERENCES, Value::Null);
        fake.script_method_missing(METHOD_IMPLEMENTATION);
        let mut client = ready_client(fake);
        let def = client.definition("/ws/lib.rs", 0, 3).unwrap();
        assert_eq!(def.len(), 1);
        assert_eq!(def[0].uri(), "file:///ws/lib.rs");
        assert_eq!(def[0].start_line(), 0);
        assert_eq!(def[0].start_character(), 3);
        assert_eq!(def[0].end_line(), 0);
        assert_eq!(def[0].end_character(), 4);
        let impls = client.implementation("/ws/lib.rs", 0, 0).unwrap();
        assert_eq!(impls.len(), 2);
        assert_eq!(impls[0].start_line(), 1);
        assert_eq!(impls[0].end_line(), 1);
        assert_eq!(impls[0].end_character(), 4);
        assert_eq!(impls[1].end_line(), 2);
        assert_eq!(impls[1].file_path().unwrap(), PathBuf::from("/ws/b.rs"));
        let refs = client.references("/ws/lib.rs", 0, 0).unwrap();
        assert!(refs.is_empty());
        let missing = client.implementation("/ws/lib.rs", 0, 0).unwrap();
        assert!(missing.is_empty());
        let inner = client.into_inner();
        assert!(inner.sent_methods().contains(&METHOD_DEFINITION));
        assert!(inner.sent_methods().contains(&METHOD_IMPLEMENTATION));
        assert!(inner.sent_methods().contains(&METHOD_REFERENCES));
        let refs_call = inner
            .sent()
            .iter()
            .find(|c| c.method == METHOD_REFERENCES)
            .unwrap();
        assert_eq!(refs_call.params["context"]["includeDeclaration"], true);
        assert!(inner
            .sent()
            .iter()
            .all(|c| !c.method.contains("filesSince") && !c.method.starts_with("$/")));
    }

    #[test]
    fn lsp_client_facade_location_link_and_invalid() {
        let mut fake = scripted_init(None);
        fake.script(
            METHOD_DEFINITION,
            json!([{
                "targetUri": "file:///ws/link.rs",
                "targetRange": {
                    "start": { "line": 4, "character": 0 },
                    "end": { "line": 8, "character": 1 }
                },
                "targetSelectionRange": {
                    "start": { "line": 5, "character": 2 },
                    "end": { "line": 5, "character": 6 }
                }
            }]),
        );
        fake.script(METHOD_DEFINITION, json!("nope"));
        fake.script_error(METHOD_REFERENCES, IdeError::lsp("transport down"));
        let mut client = ready_client(fake);
        let links = client.definition("/ws/lib.rs", 0, 0).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].uri(), "file:///ws/link.rs");
        assert_eq!(links[0].start_line(), 5);
        assert_eq!(links[0].start_character(), 2);
        assert!(client.definition("/ws/lib.rs", 0, 0).unwrap_err().is_lsp());
        assert!(client.references("/ws/lib.rs", 0, 0).unwrap_err().is_lsp());
    }

    #[test]
    fn lsp_client_facade_not_initialized_and_missing_binary() {
        let mut client = LspClient::new(FakeLsp::new());
        assert!(client.did_open("/ws/a.rs", "x").unwrap_err().is_lsp());
        assert!(client.definition("/ws/a.rs", 0, 0).unwrap_err().is_lsp());
        assert!(client.shutdown().unwrap_err().is_lsp());
        assert!(file_uri(Path::new("rel")).unwrap_err().is_not_absolute());
        assert!(LspClient::new(FakeLsp::new())
            .initialize("rel")
            .unwrap_err()
            .is_not_absolute());

        let mut missing = LspClient::new(FakeLsp::missing_binary());
        assert!(missing.initialize("/ws").unwrap_err().is_missing_binary());
        assert!(!missing.is_ready());
    }

    #[test]
    fn lsp_client_facade_shutdown_and_override_catalog() {
        let mut catalog = LanguageCatalog::new();
        catalog.override_extension("txt", "rust");
        let mut fake = scripted_init(None);
        fake.script(METHOD_SHUTDOWN, Value::Null);
        let mut client = LspClient::new(fake).with_catalog(catalog);
        client.initialize("/ws").unwrap();
        assert!(client.did_open("/ws/notes.txt", "fn x() {}").unwrap());
        assert_eq!(client.catalog().for_path("/ws/notes.txt"), "rust");
        client.shutdown().unwrap();
        assert!(!client.is_ready());
        assert!(client.open_version("/ws/notes.txt").is_none());
        let inner = client.into_inner();
        assert!(inner.sent_methods().contains(&METHOD_DID_OPEN));
        assert!(inner.sent_methods().contains(&METHOD_SHUTDOWN));
        assert!(inner.sent_methods().contains(&METHOD_EXIT));
        assert!(inner
            .sent()
            .iter()
            .any(|c| c.method == METHOD_EXIT && c.is_notification()));
    }

    #[test]
    fn lsp_location_value_object_empty_list_is_valid() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/lib.rs", "fn x() {}\n").unwrap();
        fs.add_file("/ws/other.rs", "fn y() {}\n").unwrap();
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        assert_eq!(
            LspClient::<FakeLsp>::jump(&[], &mut tabs, &mut buffers, &fs).unwrap(),
            0
        );
        assert!(tabs.is_empty());

        let a = LspLocation::new("file:///ws/lib.rs", 0, 3, 0, 4);
        let b = LspLocation::new("file:///ws/other.rs", 0, 3, 0, 4);
        assert_eq!(a.file_path().unwrap(), PathBuf::from("/ws/lib.rs"));
        assert_eq!(
            LspClient::<FakeLsp>::jump(&[a.clone(), b], &mut tabs, &mut buffers, &fs).unwrap(),
            2
        );
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.focused().unwrap().as_path(), Path::new("/ws/lib.rs"));
        assert_eq!(
            buffers.get("/ws/lib.rs").unwrap().selection(),
            Selection::new(3, 4)
        );
        assert_eq!(
            buffers.get("/ws/other.rs").unwrap().selection(),
            Selection::new(3, 4)
        );

        let again = LspLocation::new("file:///ws/lib.rs", 0, 0, 0, 2);
        again.open_or_focus(&mut tabs, &mut buffers, &fs).unwrap();
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            buffers.get("/ws/lib.rs").unwrap().selection(),
            Selection::new(0, 2)
        );
        assert_eq!(again.uri(), "file:///ws/lib.rs");
        assert!(LspLocation::new("http://example.com/x", 0, 0, 0, 0)
            .file_path()
            .unwrap_err()
            .is_lsp());
    }

    #[test]
    fn lsp_location_value_object_utf16_and_file_uri() {
        let text = "let café = 1;\nnext\n";
        assert_eq!(position_at(text, 0), (0, 0));
        let cafe_offset = "let ".chars().count();
        assert_eq!(position_at(text, cafe_offset), (0, 4));
        assert_eq!(position_at(text, text.chars().count()), (2, 0));
        assert_eq!(position_at("hi", 99), (0, 2));
        let loc = LspLocation::new("file:///ws/café.rs", 0, 4, 0, 8);
        let sel = loc.to_selection(text);
        assert_eq!(sel.start(), cafe_offset);
        assert_eq!(
            path_from_file_uri("file:///ws/a.rs").unwrap(),
            PathBuf::from("/ws/a.rs")
        );
        assert_eq!(
            path_from_file_uri("file://localhost/ws/a.rs").unwrap(),
            PathBuf::from("/ws/a.rs")
        );
        assert_eq!(
            path_from_file_uri("file:///ws/my%20file.rs").unwrap(),
            PathBuf::from("/ws/my file.rs")
        );
        assert_eq!(
            path_from_file_uri("file:///ws/a.rs?x=1#frag").unwrap(),
            PathBuf::from("/ws/a.rs")
        );
        assert_eq!(file_uri(Path::new("/ws/a.rs")).unwrap(), "file:///ws/a.rs");
        assert_eq!(percent_decode("%2F"), "/");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
        assert_eq!(percent_decode("A%2fb"), "A/b");
        assert_eq!(hex_val(b'0'), Some(0));
        assert_eq!(hex_val(b'9'), Some(9));
        assert_eq!(hex_val(b'a'), Some(10));
        assert_eq!(hex_val(b'F'), Some(15));
        assert_eq!(hex_val(b'g'), None);
        assert_eq!(incremental_edit("abc", "abc"), (0, 3, 0, 3, String::new()));
        assert_eq!(incremental_edit("abc", "aXc"), (0, 1, 0, 2, "X".into()));
        assert_eq!(incremental_edit("", "hi"), (0, 0, 0, 0, "hi".into()));
        assert_eq!(offset_at("a\nb", 1, 0), 2);
        assert_eq!(offset_at("a\nb", 0, 1), 1);
        assert_eq!(offset_at("ab\ncd", 1, 1), 4);
        assert_eq!(offset_at("a\nb", 5, 0), 3);
        assert_eq!(offset_at("a\nb", 0, 99), 1);
    }

    #[test]
    fn lsp_client_facade_transport_accessors() {
        let client = LspClient::new(FakeLsp::new());
        assert_eq!(client.transport().sent().len(), 0);
        let mut client = client;
        client
            .transport_mut()
            .script(METHOD_INITIALIZE, init_result(None));
        client.initialize("/ws").unwrap();
        assert_eq!(client.transport().sent().len(), 2);
    }
}
