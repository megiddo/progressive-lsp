//! `DiscoverCommand`: definition / implementation / references at the focused
//! cursor, then the same [`LspClient::jump`] the UI uses.

use crate::buffer::BufferMap;
use crate::error::IdeError;
use crate::log::RunLog;
use crate::lsp::{position_at, LspClient};
use crate::ports::{FsPort, LspTransport};
use crate::tabs::TabStrip;

/// Resolver action. Maps 1:1 onto a stock LSP method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscoverKind {
    Definition,
    Implementation,
    References,
}

impl DiscoverKind {
    pub fn lsp_method(self) -> &'static str {
        match self {
            Self::Definition => "textDocument/definition",
            Self::Implementation => "textDocument/implementation",
            Self::References => "textDocument/references",
        }
    }
}

/// Command: focused tab + cursor → LSP discover + jump. No egui.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoverCommand {
    kind: DiscoverKind,
}

impl DiscoverCommand {
    pub fn new(kind: DiscoverKind) -> Self {
        Self { kind }
    }

    pub fn definition() -> Self {
        Self::new(DiscoverKind::Definition)
    }

    pub fn implementation() -> Self {
        Self::new(DiscoverKind::Implementation)
    }

    pub fn references() -> Self {
        Self::new(DiscoverKind::References)
    }

    pub fn kind(self) -> DiscoverKind {
        self.kind
    }

    pub fn lsp_method(self) -> &'static str {
        self.kind.lsp_method()
    }

    /// Same jump as Navigate / F12 / the editor context menu.
    ///
    /// No focused buffer → [`IdeError::NoFileOpen`]. No client →
    /// [`IdeError::MissingBinary`]. Empty location list is valid (`Ok(0)`).
    pub fn apply<T: LspTransport>(
        &self,
        lsp: Option<&mut LspClient<T>>,
        tabs: &mut TabStrip,
        buffers: &mut BufferMap,
        fs: &impl FsPort,
        run_log: Option<&mut RunLog>,
    ) -> Result<usize, IdeError> {
        let Some(id) = tabs.focused().cloned() else {
            return Err(IdeError::NoFileOpen);
        };
        let Some(buf) = buffers.get(id.as_path()) else {
            return Err(IdeError::NoFileOpen);
        };
        let (line, character) = position_at(&buf.text(), buf.selection().start());
        let path = buf.path().to_path_buf();
        let method = self.lsp_method();
        let uri = crate::lsp::file_uri(&path).unwrap_or_else(|_| String::new());
        let Some(client) = lsp else {
            if let Some(log) = run_log {
                log.log_discover(
                    method,
                    &path,
                    &uri,
                    line,
                    character,
                    None,
                    Some(&IdeError::MissingBinary.to_string()),
                );
            }
            return Err(IdeError::MissingBinary);
        };
        let result = match self.kind {
            DiscoverKind::Definition => client.definition(&path, line, character),
            DiscoverKind::Implementation => client.implementation(&path, line, character),
            DiscoverKind::References => client.references(&path, line, character),
        };
        match result {
            Ok(locations) => {
                if let Some(log) = run_log {
                    log.log_discover(
                        method,
                        &path,
                        &uri,
                        line,
                        character,
                        Some(locations.len() as u64),
                        None,
                    );
                }
                LspClient::<T>::jump(&locations, tabs, buffers, fs)
            }
            Err(e) => {
                if let Some(log) = run_log {
                    log.log_discover(
                        method,
                        &path,
                        &uri,
                        line,
                        character,
                        None,
                        Some(&e.to_string()),
                    );
                }
                Err(e)
            }
        }
    }
}

/// Command / value: a Navigate (or F12) click records a kind; apply runs
/// [`DiscoverCommand`] once after the menu has closed. Close is not this type —
/// recording survives menu teardown with no panic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingDiscover {
    kind: DiscoverKind,
}

impl PendingDiscover {
    pub fn record(kind: DiscoverKind) -> Self {
        Self { kind }
    }

    pub fn kind(self) -> DiscoverKind {
        self.kind
    }

    /// Apply after the menu UI (same frame after close, or next frame).
    pub fn apply<T: LspTransport>(
        &self,
        lsp: Option<&mut LspClient<T>>,
        tabs: &mut TabStrip,
        buffers: &mut BufferMap,
        fs: &impl FsPort,
        run_log: Option<&mut RunLog>,
    ) -> Result<usize, IdeError> {
        DiscoverCommand::new(self.kind).apply(lsp, tabs, buffers, fs, run_log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Selection;
    use crate::log::LogCategory;
    use crate::ports::{FakeClock, FakeLsp, MemFs};
    use crate::LspClient;
    use serde_json::{json, Value};

    fn location_json(uri: &str, sl: u32, sc: u32, el: u32, ec: u32) -> Value {
        json!({
            "uri": uri,
            "range": {
                "start": { "line": sl, "character": sc },
                "end": { "line": el, "character": ec }
            }
        })
    }

    fn scripted_init() -> FakeLsp {
        let mut fake = FakeLsp::new();
        fake.script(
            "initialize",
            json!({
                "capabilities": { "experimental": { "progressiveLsp": { "version": "v1" } } }
            }),
        );
        fake
    }

    fn ready(fake: FakeLsp) -> LspClient<FakeLsp> {
        let mut client = LspClient::new(fake);
        client.initialize("/ws").unwrap();
        client
    }

    fn open_lib() -> (MemFs, TabStrip, BufferMap) {
        let mut fs = MemFs::new();
        fs.add_file("/ws/lib.rs", "fn x() {}\n").unwrap();
        fs.add_file("/ws/other.rs", "fn y() {}\n").unwrap();
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        buffers.open("/ws/lib.rs", &fs).unwrap();
        tabs.open("/ws/lib.rs");
        (fs, tabs, buffers)
    }

    #[test]
    fn discover_kind_value_object_maps_each_kind_to_lsp_method() {
        assert_eq!(
            DiscoverKind::Definition.lsp_method(),
            "textDocument/definition"
        );
        assert_eq!(
            DiscoverKind::Implementation.lsp_method(),
            "textDocument/implementation"
        );
        assert_eq!(
            DiscoverKind::References.lsp_method(),
            "textDocument/references"
        );
        assert_eq!(
            DiscoverCommand::definition().lsp_method(),
            DiscoverKind::Definition.lsp_method()
        );
        assert_eq!(
            DiscoverCommand::implementation().kind(),
            DiscoverKind::Implementation
        );
        assert_eq!(
            DiscoverCommand::references().kind(),
            DiscoverKind::References
        );
        assert_eq!(
            DiscoverCommand::new(DiscoverKind::Definition),
            DiscoverCommand::definition()
        );
    }

    #[test]
    fn discover_command_each_kind_sends_matching_lsp_method() {
        let mut fake = scripted_init();
        fake.script(
            "textDocument/definition",
            location_json("file:///ws/other.rs", 0, 3, 0, 4),
        );
        fake.script(
            "textDocument/implementation",
            location_json("file:///ws/other.rs", 0, 3, 0, 4),
        );
        fake.script("textDocument/references", Value::Null);
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        buffers
            .get_mut("/ws/lib.rs")
            .unwrap()
            .set_selection(Selection::collapsed(3));

        DiscoverCommand::definition()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();
        DiscoverCommand::implementation()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();
        DiscoverCommand::references()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();

        let sent = client.transport().sent();
        let methods: Vec<&str> = sent.iter().map(|c| c.method.as_str()).collect();
        assert!(methods.contains(&"textDocument/definition"));
        assert!(methods.contains(&"textDocument/implementation"));
        assert!(methods.contains(&"textDocument/references"));
        let def = sent
            .iter()
            .find(|c| c.method == "textDocument/definition")
            .unwrap();
        assert_eq!(def.params["position"]["line"], 0);
        assert_eq!(def.params["position"]["character"], 3);
        assert_eq!(def.params["textDocument"]["uri"], "file:///ws/lib.rs");
        let refs = sent
            .iter()
            .find(|c| c.method == "textDocument/references")
            .unwrap();
        assert_eq!(refs.params["context"]["includeDeclaration"], true);
    }

    #[test]
    fn discover_command_jump_opens_and_focuses_location_tab() {
        let mut fake = scripted_init();
        fake.script(
            "textDocument/definition",
            location_json("file:///ws/other.rs", 0, 3, 0, 4),
        );
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        let jumped = DiscoverCommand::definition()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();
        assert_eq!(jumped, 1);
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            tabs.focused().unwrap().as_path(),
            std::path::Path::new("/ws/other.rs")
        );
        assert_eq!(
            buffers.get("/ws/other.rs").unwrap().selection(),
            Selection::new(3, 4)
        );
    }

    #[test]
    fn discover_command_empty_location_list_is_valid() {
        let mut fake = scripted_init();
        fake.script("textDocument/definition", Value::Null);
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        let jumped = DiscoverCommand::definition()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();
        assert_eq!(jumped, 0);
        assert_eq!(tabs.len(), 1);
        assert_eq!(
            tabs.focused().unwrap().as_path(),
            std::path::Path::new("/ws/lib.rs")
        );
    }

    #[test]
    fn discover_command_no_focused_buffer_is_domain_error() {
        let fake = scripted_init();
        let mut client = ready(fake);
        let fs = MemFs::new();
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        let err = DiscoverCommand::definition()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap_err();
        assert!(err.is_no_file_open());
        assert_eq!(err.to_string(), "No file open");
        assert!(client
            .transport()
            .sent()
            .iter()
            .all(|c| c.method != "textDocument/definition"));

        tabs.open("/ws/ghost.rs");
        let missing_buf = DiscoverCommand::implementation()
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap_err();
        assert!(missing_buf.is_no_file_open());
    }

    #[test]
    fn discover_command_missing_client_is_missing_binary() {
        let (fs, mut tabs, mut buffers) = open_lib();
        let err = DiscoverCommand::references()
            .apply(
                None::<&mut LspClient<FakeLsp>>,
                &mut tabs,
                &mut buffers,
                &fs,
                None,
            )
            .unwrap_err();
        assert!(err.is_missing_binary());
        assert_eq!(err.to_string(), "progressive-lsp binary not found");
    }

    #[test]
    fn discover_command_existing_lsp_error_is_domain_error() {
        let mut fake = scripted_init();
        fake.script_error("textDocument/definition", IdeError::lsp("eof"));
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        let mut log = RunLog::memory(FakeClock::at_unix_ms(1)).unwrap();
        let err = DiscoverCommand::definition()
            .apply(
                Some(&mut client),
                &mut tabs,
                &mut buffers,
                &fs,
                Some(&mut log),
            )
            .unwrap_err();
        assert!(err.is_lsp());
        assert_eq!(err.to_string(), "lsp: eof");
        let lsp_rows: Vec<_> = log
            .rows()
            .unwrap()
            .into_iter()
            .filter(|r| r.category() == LogCategory::Lsp)
            .collect();
        assert_eq!(lsp_rows.len(), 1);
        assert_eq!(lsp_rows[0].event(), "textDocument/definition");
        assert_eq!(lsp_rows[0].payload().unwrap()["error"], json!("lsp: eof"));
        assert_eq!(lsp_rows[0].payload().unwrap()["path"], "/ws/lib.rs");
        assert_eq!(lsp_rows[0].payload().unwrap()["uri"], "file:///ws/lib.rs");
        assert_eq!(lsp_rows[0].payload().unwrap()["line"], 0);
        assert_eq!(lsp_rows[0].payload().unwrap()["character"], 0);
        assert_eq!(
            lsp_rows[0].payload().unwrap()["location_count"],
            Value::Null
        );
    }

    #[test]
    fn discover_command_logs_success_via_run_log() {
        let mut fake = scripted_init();
        fake.script("textDocument/implementation", Value::Null);
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        let mut log = RunLog::memory(FakeClock::at_unix_ms(2)).unwrap();
        DiscoverCommand::implementation()
            .apply(
                Some(&mut client),
                &mut tabs,
                &mut buffers,
                &fs,
                Some(&mut log),
            )
            .unwrap();
        let lsp_rows: Vec<_> = log
            .rows()
            .unwrap()
            .into_iter()
            .filter(|r| r.category() == LogCategory::Lsp)
            .collect();
        assert_eq!(lsp_rows.len(), 1);
        assert_eq!(lsp_rows[0].event(), "textDocument/implementation");
        assert_eq!(lsp_rows[0].payload().unwrap()["error"], Value::Null);
        assert_eq!(lsp_rows[0].payload().unwrap()["path"], "/ws/lib.rs");
        assert_eq!(lsp_rows[0].payload().unwrap()["uri"], "file:///ws/lib.rs");
        assert_eq!(lsp_rows[0].payload().unwrap()["line"], 0);
        assert_eq!(lsp_rows[0].payload().unwrap()["character"], 0);
        assert_eq!(lsp_rows[0].payload().unwrap()["location_count"], 0);
    }

    #[test]
    fn discover_command_logs_missing_client() {
        let (fs, mut tabs, mut buffers) = open_lib();
        let mut log = RunLog::memory(FakeClock::at_unix_ms(3)).unwrap();
        let err = DiscoverCommand::definition()
            .apply(
                None::<&mut LspClient<FakeLsp>>,
                &mut tabs,
                &mut buffers,
                &fs,
                Some(&mut log),
            )
            .unwrap_err();
        assert!(err.is_missing_binary());
        let lsp_rows: Vec<_> = log
            .rows()
            .unwrap()
            .into_iter()
            .filter(|r| r.category() == LogCategory::Lsp)
            .collect();
        assert_eq!(lsp_rows.len(), 1);
        assert_eq!(lsp_rows[0].event(), "textDocument/definition");
        assert!(lsp_rows[0].payload().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("binary"));
        assert_eq!(
            lsp_rows[0].payload().unwrap()["location_count"],
            Value::Null
        );
    }

    #[test]
    fn pending_discover_records_kind_apply_runs_once_and_menu_close_does_not_panic() {
        let click = PendingDiscover::record(DiscoverKind::Definition);
        assert_eq!(click.kind(), DiscoverKind::Definition);
        assert_eq!(
            PendingDiscover::record(DiscoverKind::Implementation).kind(),
            DiscoverKind::Implementation
        );
        assert_eq!(
            PendingDiscover::record(DiscoverKind::References).kind(),
            DiscoverKind::References
        );
        assert_eq!(
            PendingDiscover::record(DiscoverKind::Definition),
            PendingDiscover::record(DiscoverKind::Definition)
        );
        assert_ne!(
            PendingDiscover::record(DiscoverKind::Definition),
            PendingDiscover::record(DiscoverKind::References)
        );

        // Menu close is not this Command: the recorded value is Copy and has no
        // Ui to unwind, so close cannot panic apply.
        let pending = click;

        let mut fake = scripted_init();
        fake.script(
            "textDocument/definition",
            location_json("file:///ws/other.rs", 0, 3, 0, 4),
        );
        let mut client = ready(fake);
        let (fs, mut tabs, mut buffers) = open_lib();
        buffers
            .get_mut("/ws/lib.rs")
            .unwrap()
            .set_selection(Selection::collapsed(3));

        let jumped = pending
            .apply(Some(&mut client), &mut tabs, &mut buffers, &fs, None)
            .unwrap();
        assert_eq!(jumped, 1);
        assert_eq!(
            tabs.focused().unwrap().as_path(),
            std::path::Path::new("/ws/other.rs")
        );
        assert_eq!(
            buffers.get("/ws/other.rs").unwrap().selection(),
            Selection::new(3, 4)
        );
        let defs = client
            .transport()
            .sent()
            .iter()
            .filter(|c| c.method == "textDocument/definition")
            .count();
        assert_eq!(defs, 1);
        let def = client
            .transport()
            .sent()
            .iter()
            .find(|c| c.method == "textDocument/definition")
            .unwrap();
        assert_eq!(def.params["position"]["line"], 0);
        assert_eq!(def.params["position"]["character"], 3);
        assert_ne!(def.params["position"]["character"], 0);
    }
}
