//! eframe view. Ignored by llvm-cov. Domain state lives in the lib.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::text::LayoutJob;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use poc_ide::{
    advertised_control_socket, position_at, BufferMap, ClipboardPort, ConflictChoice,
    ControlClient, DialogPort, DiskEvent, DiskWatch, EditCommand, FileTree, FsPort, HighlightSpan,
    Highlighter, IdeError, LayoutState, LspClient, NotifyWatch, OpenBuffer, ProtocolConsole,
    RunLog, Selection, ServeMode, SpawnSpec, StdFs, StdioLsp, TabId, TabStrip, TranscriptKind,
    TreeNode, UnixControl, WatchPort, WorkspaceRoot, CONTROL_UNARY_METHODS, STOCK_LSP_METHODS,
};
use serde_json::json;
use std::sync::mpsc;

/// Native `rfd` Adapter. Tests never construct this type.
pub struct RfdDialog;

impl DialogPort for RfdDialog {
    fn open_folder(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_folder()
    }

    fn open_file(&mut self) -> Option<PathBuf> {
        rfd::FileDialog::new().pick_file()
    }
}

/// Production `WatchPort` Adapter. Owns the `notify` OS watcher; mapping stays on
/// [`NotifyWatch`]. Tests never construct this type.
struct LiveWatch {
    watcher: Option<RecommendedWatcher>,
    mapped: NotifyWatch,
    root: Option<PathBuf>,
}

impl LiveWatch {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })
        .ok();
        Self {
            watcher,
            mapped: NotifyWatch::from_receiver(rx),
            root: None,
        }
    }

    fn watch_root(&mut self, path: &Path) -> Result<(), IdeError> {
        if let Some(old) = self.root.take() {
            self.unwatch(&old);
        }
        self.watch(path)?;
        self.root = Some(path.to_path_buf());
        Ok(())
    }
}

impl WatchPort for LiveWatch {
    fn watch(&mut self, path: &Path) -> Result<(), IdeError> {
        if let Some(watcher) = &mut self.watcher {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(|e| IdeError::watch(e.to_string()))?;
        }
        self.mapped.watch(path)
    }

    fn unwatch(&mut self, path: &Path) {
        if let Some(watcher) = &mut self.watcher {
            let _ = watcher.unwatch(path);
        }
        self.mapped.unwatch(path);
    }

    fn poll(&mut self) -> Vec<DiskEvent> {
        self.mapped.poll()
    }
}

/// Production `ClipboardPort` Adapter. Lives in the bin so tests use `FakeClipboard`.
pub struct ArboardClipboard;

impl ClipboardPort for ArboardClipboard {
    fn get_text(&mut self) -> Result<String, IdeError> {
        let mut clip = arboard::Clipboard::new().map_err(|e| IdeError::clipboard(e.to_string()))?;
        clip.get_text()
            .map_err(|e| IdeError::clipboard(e.to_string()))
    }

    fn set_text(&mut self, text: &str) -> Result<(), IdeError> {
        let mut clip = arboard::Clipboard::new().map_err(|e| IdeError::clipboard(e.to_string()))?;
        clip.set_text(text.to_string())
            .map_err(|e| IdeError::clipboard(e.to_string()))
    }
}

pub struct PocIdeApp {
    dialog: RfdDialog,
    fs: StdFs,
    clipboard: ArboardClipboard,
    root: Option<WorkspaceRoot>,
    tree: Option<FileTree>,
    tabs: TabStrip,
    buffers: BufferMap,
    highlighter: Highlighter,
    layout: LayoutState,
    watch: LiveWatch,
    disk: DiskWatch,
    lsp: Option<LspClient<StdioLsp>>,
    lsp_error: Option<String>,
    serve_mode: ServeMode,
    control_socket_path: Option<PathBuf>,
    control: Option<ControlClient<UnixControl>>,
    control_error: Option<String>,
    console: ProtocolConsole,
    console_open: bool,
    console_control_tab: bool,
    console_method: String,
    console_body: String,
    status: String,
    run_log: RunLog,
}

impl PocIdeApp {
    pub fn new(
        folder: Option<PathBuf>,
        file: Option<PathBuf>,
        control_socket: Option<PathBuf>,
        run_log: RunLog,
    ) -> Self {
        let serve_mode = if control_socket.is_some() {
            ServeMode::ControlSocket
        } else {
            ServeMode::StockStdio
        };
        let console_method = if serve_mode.is_control_socket() {
            CONTROL_UNARY_METHODS[0].to_string()
        } else {
            STOCK_LSP_METHODS[0].to_string()
        };
        let mut app = Self {
            dialog: RfdDialog,
            fs: StdFs,
            clipboard: ArboardClipboard,
            root: None,
            tree: None,
            tabs: TabStrip::new(),
            buffers: BufferMap::new(),
            highlighter: Highlighter::new(),
            layout: LayoutState::new(),
            watch: LiveWatch::new(),
            disk: DiskWatch::new(),
            lsp: None,
            lsp_error: None,
            serve_mode,
            control_socket_path: control_socket,
            control: None,
            control_error: None,
            console: ProtocolConsole::new(),
            console_open: true,
            console_control_tab: serve_mode.is_control_socket(),
            console_method,
            console_body: String::new(),
            status: String::new(),
            run_log,
        };
        if let Some(dir) = folder {
            app.apply_folder_path(&dir);
        }
        if let Some(path) = file {
            app.apply_file_path(&path);
        }
        app
    }

    fn apply_folder_path(&mut self, path: &Path) {
        match WorkspaceRoot::from_folder_path(path, &self.fs) {
            Ok(root) => {
                self.run_log.log_open_folder(root.as_path());
                self.set_root(root, None);
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn apply_file_path(&mut self, path: &Path) {
        match WorkspaceRoot::from_file_path(path, &self.fs) {
            Ok((root, file)) => {
                self.run_log.log_open_file(&file);
                self.set_root(root, Some(file));
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn set_root(&mut self, root: WorkspaceRoot, open_file: Option<PathBuf>) {
        match self.fs.read_tree(&root) {
            Ok(tree) => {
                self.run_log
                    .log_tree_load(root.as_path(), tree.children().len(), None);
                if self.root.as_ref() != Some(&root) {
                    self.tabs = TabStrip::new();
                    self.buffers = BufferMap::new();
                    self.disk = DiskWatch::new();
                    self.shutdown_lsp();
                }
                let watch_err = self.watch.watch_root(root.as_path()).err();
                self.connect_lsp(&root);
                self.root = Some(root);
                self.tree = Some(tree);
                self.status = watch_err
                    .map(|e| e.to_string())
                    .or_else(|| self.lsp_error.clone())
                    .unwrap_or_default();
                if let Some(file) = open_file {
                    self.open_path(&file);
                }
            }
            Err(e) => {
                self.run_log
                    .log_tree_load(root.as_path(), 0, Some(&e.to_string()));
                self.status = e.to_string();
            }
        }
    }

    fn pick_folder(&mut self) {
        if let Ok(Some(root)) = WorkspaceRoot::open_folder(&mut self.dialog, &self.fs) {
            self.run_log.log_open_folder(root.as_path());
            self.set_root(root, None);
        }
    }

    fn pick_file(&mut self) {
        if let Ok(Some((root, file))) = WorkspaceRoot::open_file(&mut self.dialog, &self.fs) {
            self.run_log.log_open_file(&file);
            self.set_root(root, Some(file));
        }
    }

    fn open_path(&mut self, path: &Path) {
        match self.buffers.open(path, &self.fs) {
            Ok(buf) => {
                let opened = buf.path().to_path_buf();
                let text = buf.text();
                self.tabs.open(&opened);
                self.run_log.log_tab_open(&opened);
                if let Some(lsp) = &mut self.lsp {
                    match lsp.did_open(&opened, &text) {
                        Ok(_) => self.run_log.log_lsp("textDocument/didOpen", None),
                        Err(e) => {
                            self.run_log
                                .log_lsp("textDocument/didOpen", Some(&e.to_string()));
                            self.status = e.to_string();
                            return;
                        }
                    }
                }
                self.status.clear();
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn close_tab(&mut self, id: &TabId) {
        if let Some(lsp) = &mut self.lsp {
            let _ = lsp.did_close(id.as_path());
        }
        self.buffers.close(id.as_path());
        self.tabs.close(id);
        self.run_log.log_tab_close(id.as_path());
    }

    fn save_focused(&mut self) {
        let Some(id) = self.tabs.focused().cloned() else {
            return;
        };
        if let Some(buf) = self.buffers.get_mut(id.as_path()) {
            match buf.save(&mut self.fs) {
                Ok(()) => {
                    self.run_log.log_save(id.as_path(), None);
                    if let Some(lsp) = &mut self.lsp {
                        match lsp.did_save(id.as_path()) {
                            Ok(()) => self.run_log.log_lsp("textDocument/didSave", None),
                            Err(e) => {
                                self.run_log
                                    .log_lsp("textDocument/didSave", Some(&e.to_string()));
                                self.status = e.to_string();
                                return;
                            }
                        }
                    }
                    self.status.clear();
                }
                Err(e) => {
                    self.run_log.log_save(id.as_path(), Some(&e.to_string()));
                    self.status = e.to_string();
                }
            }
        }
    }

    fn connect_lsp(&mut self, root: &WorkspaceRoot) {
        self.shutdown_lsp();
        match SpawnSpec::resolve() {
            Ok(spec) => {
                match StdioLsp::spawn_serve(
                    &spec,
                    self.serve_mode,
                    self.control_socket_path.as_deref(),
                ) {
                    Ok(transport) => {
                        let mut client = LspClient::new(transport).with_mode(self.serve_mode);
                        match client.initialize(root.as_path()) {
                            Ok(()) => {
                                self.run_log.log_lsp("initialize", None);
                                self.connect_control(client.progressive_cap());
                                self.lsp = Some(client);
                                self.lsp_error = None;
                            }
                            Err(e) => {
                                self.run_log.log_lsp("initialize", Some(&e.to_string()));
                                self.lsp = None;
                                self.lsp_error = Some(e.to_string());
                            }
                        }
                    }
                    Err(e) => {
                        self.run_log.log_lsp("initialize", Some(&e.to_string()));
                        self.lsp = None;
                        self.lsp_error = Some(e.to_string());
                        if self.serve_mode.is_control_socket() {
                            self.control_error = Some(e.to_string());
                        }
                    }
                }
            }
            Err(e) => {
                self.run_log.log_lsp("initialize", Some(&e.to_string()));
                self.lsp = None;
                self.lsp_error = Some(e.to_string());
                if self.serve_mode.is_control_socket() {
                    self.control_error = Some(e.to_string());
                }
            }
        }
    }

    fn connect_control(&mut self, cap: Option<&poc_ide::ProgressiveLspCap>) {
        self.control = None;
        self.control_error = None;
        if !self.serve_mode.is_control_socket() {
            return;
        }
        let Some(cap) = cap else {
            let err = IdeError::control_socket_missing();
            self.run_log.log_control_connect_error(&err.to_string());
            self.control_error = Some(err.to_string());
            self.console.record_control_unavailable(&err);
            return;
        };
        match advertised_control_socket(cap) {
            Ok(path) => match ControlClient::<UnixControl>::connect(path) {
                Ok(client) => {
                    self.control = Some(client);
                    self.control_error = None;
                }
                Err(e) => {
                    self.run_log.log_control_connect_error(&e.to_string());
                    self.control_error = Some(e.to_string());
                    self.console.record_control_unavailable(&e);
                }
            },
            Err(e) => {
                self.run_log.log_control_connect_error(&e.to_string());
                self.control_error = Some(e.to_string());
                self.console.record_control_unavailable(&e);
            }
        }
    }

    fn shutdown_lsp(&mut self) {
        self.control = None;
        if let Some(mut client) = self.lsp.take() {
            let _ = client.shutdown();
        }
    }

    fn missing_server_status(&self) -> String {
        self.lsp_error
            .clone()
            .unwrap_or_else(|| IdeError::MissingBinary.to_string())
    }

    fn discover(&mut self, kind: DiscoverKind) {
        let Some(id) = self.tabs.focused().cloned() else {
            self.status = "No file open".into();
            return;
        };
        let Some(buf) = self.buffers.get(id.as_path()) else {
            self.status = "No file open".into();
            return;
        };
        let (line, character) = position_at(&buf.text(), buf.selection().start());
        let path = buf.path().to_path_buf();
        let Some(lsp) = self.lsp.as_mut() else {
            self.status = self.missing_server_status();
            return;
        };
        let method = match kind {
            DiscoverKind::Definition => "textDocument/definition",
            DiscoverKind::Implementation => "textDocument/implementation",
            DiscoverKind::References => "textDocument/references",
        };
        let result = match kind {
            DiscoverKind::Definition => lsp.definition(&path, line, character),
            DiscoverKind::Implementation => lsp.implementation(&path, line, character),
            DiscoverKind::References => lsp.references(&path, line, character),
        };
        match result {
            Ok(locations) => {
                self.run_log.log_lsp(method, None);
                match LspClient::<StdioLsp>::jump(
                    &locations,
                    &mut self.tabs,
                    &mut self.buffers,
                    &self.fs,
                ) {
                    Ok(0) => self.status = "No locations".into(),
                    Ok(_) => self.status.clear(),
                    Err(e) => self.status = e.to_string(),
                }
            }
            Err(e) => {
                self.run_log.log_lsp(method, Some(&e.to_string()));
                self.status = e.to_string();
            }
        }
    }

    fn send_console(&mut self) {
        let method = self.console_method.clone();
        if self.console_control_tab {
            if let Some(client) = self.control.as_mut() {
                let body = ProtocolConsole::encode_body(&method, &self.console_body);
                if let Err(e) = self.console.send_control(client, &method, body) {
                    self.status = e.to_string();
                } else {
                    self.status.clear();
                }
            } else {
                let err = self
                    .control_error
                    .as_deref()
                    .map(IdeError::control)
                    .unwrap_or_else(IdeError::control_socket_missing);
                self.console.record_control_unavailable(&err);
                self.status = err.to_string();
            }
        } else {
            let params = serde_json::from_str(&self.console_body).unwrap_or_else(|_| json!({}));
            let Some(lsp) = self.lsp.as_mut() else {
                self.status = self.missing_server_status();
                return;
            };
            if let Err(e) = self.console.send_lsp(lsp.transport_mut(), &method, params) {
                self.status = e.to_string();
            } else {
                self.status.clear();
            }
        }
    }

    fn show_console_panel(&mut self, ui: &mut egui::Ui) {
        if let Some(client) = self.control.as_mut() {
            if let Err(e) = self.console.drain_pushes(client) {
                self.status = e.to_string();
            }
        }
        if !self.console_open {
            egui::Panel::bottom("console_bar")
                .resizable(false)
                .show(ui, |ui| {
                    if ui.button("Protocol console").clicked() {
                        self.console_open = true;
                    }
                });
            return;
        }
        egui::Panel::bottom("console")
            .resizable(true)
            .default_size(180.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Protocol console");
                    if ui.small_button("×").clicked() {
                        self.console_open = false;
                    }
                    ui.selectable_value(&mut self.console_control_tab, false, "LSP");
                    ui.selectable_value(&mut self.console_control_tab, true, "Control");
                });
                if self.console_control_tab {
                    if let Some(err) = &self.control_error {
                        ui.colored_label(egui::Color32::from_rgb(200, 80, 80), err);
                    } else if self.control.is_none() {
                        ui.colored_label(
                            egui::Color32::from_rgb(200, 80, 80),
                            IdeError::control_socket_missing().to_string(),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    let methods: &[&str] = if self.console_control_tab {
                        CONTROL_UNARY_METHODS
                    } else {
                        STOCK_LSP_METHODS
                    };
                    egui::ComboBox::from_id_salt("console_method")
                        .selected_text(&self.console_method)
                        .show_ui(ui, |ui| {
                            for m in methods {
                                ui.selectable_value(&mut self.console_method, (*m).to_string(), *m);
                            }
                        });
                    ui.add(
                        egui::TextEdit::singleline(&mut self.console_method)
                            .desired_width(220.0)
                            .hint_text("method"),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.console_body)
                            .desired_width(280.0)
                            .hint_text(if self.console_control_tab {
                                "body (SetConfig TOML / packs / generation)"
                            } else {
                                "JSON params"
                            }),
                    );
                    if ui.button("Send").clicked() {
                        self.send_console();
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if self.console.is_empty() {
                            ui.weak("No transcript yet.");
                        } else {
                            for entry in self.console.entries() {
                                let color = match entry.kind() {
                                    TranscriptKind::LspError | TranscriptKind::ControlError => {
                                        egui::Color32::from_rgb(200, 80, 80)
                                    }
                                    TranscriptKind::ControlPush => {
                                        egui::Color32::from_rgb(80, 160, 80)
                                    }
                                    _ => ui.visuals().text_color(),
                                };
                                ui.colored_label(
                                    color,
                                    format!(
                                        "[{}] {} id={} {}",
                                        entry.kind().as_str(),
                                        entry.method(),
                                        entry.request_id(),
                                        entry.body()
                                    ),
                                );
                            }
                        }
                    });
            });
    }

    fn show_conflict_modal(&mut self, ui: &mut egui::Ui) {
        let Some(modal) = self.disk.first_pending().cloned() else {
            return;
        };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("disk_conflict")).show(ui.ctx(), |ui| {
            ui.heading("File changed on disk");
            ui.label(format!(
                "{} changed on disk. Load from disk or keep in memory?",
                modal.path().display()
            ));
            ui.horizontal(|ui| {
                if ui.button("Load from disk").clicked() {
                    choice = Some(ConflictChoice::LoadDisk);
                }
                if ui.button("Keep in memory").clicked() {
                    choice = Some(ConflictChoice::KeepMemory);
                }
            });
        });
        if let Some(choice) = choice {
            self.run_log.log_conflict_resolve(modal.path(), choice);
            if let Err(e) = self
                .disk
                .resolve(modal.path(), choice, &mut self.buffers, &self.fs)
            {
                self.status = e.to_string();
            }
        }
    }
}

impl eframe::App for PocIdeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let already: Vec<PathBuf> = self
            .disk
            .pending()
            .iter()
            .map(|m| m.path().to_path_buf())
            .collect();
        self.disk.ingest(&mut self.watch, &self.buffers);
        let newly: Vec<(PathBuf, u64)> = self
            .disk
            .pending()
            .iter()
            .filter(|m| !already.iter().any(|p| p == m.path()))
            .map(|m| (m.path().to_path_buf(), m.mtime()))
            .collect();
        for (path, mtime) in newly {
            self.run_log.log_conflict_enqueue(&path, mtime);
        }
        self.show_conflict_modal(ui);
        self.show_console_panel(ui);

        if ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command) {
            self.save_focused();
        }
        if ui.input(|i| i.key_pressed(egui::Key::F12) && i.modifiers.shift) {
            self.discover(DiscoverKind::References);
        } else if ui.input(|i| i.key_pressed(egui::Key::F12) && i.modifiers.command) {
            self.discover(DiscoverKind::Implementation);
        } else if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.discover(DiscoverKind::Definition);
        }

        egui::Panel::top("menu").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Folder…").clicked() {
                        ui.close();
                        self.pick_folder();
                    }
                    if ui.button("Open File…").clicked() {
                        ui.close();
                        self.pick_file();
                    }
                    if ui.button("Save").clicked() {
                        ui.close();
                        self.save_focused();
                    }
                });
                ui.menu_button("Navigate", |ui| {
                    if ui.button("Go to Definition").clicked() {
                        ui.close();
                        self.discover(DiscoverKind::Definition);
                    }
                    if ui.button("Go to Implementation").clicked() {
                        ui.close();
                        self.discover(DiscoverKind::Implementation);
                    }
                    if ui.button("Find References").clicked() {
                        ui.close();
                        self.discover(DiscoverKind::References);
                    }
                });
                if !self.status.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(200, 80, 80), &self.status);
                }
            });
        });

        let tree_response = egui::Panel::left("tree")
            .resizable(true)
            .default_size(self.layout.left_width())
            .min_size(LayoutState::MIN_LEFT_WIDTH)
            .max_size(LayoutState::MAX_LEFT_WIDTH)
            .show(ui, |ui| {
                ui.heading("Workspace");
                let root_label = self
                    .root
                    .as_ref()
                    .map(|root| root.as_path().display().to_string());
                let children = self.tree.as_ref().map(|tree| tree.children().to_vec());
                match (root_label, children) {
                    (Some(label), Some(nodes)) => {
                        ui.label(label);
                        ui.separator();
                        let mut clicked = None;
                        let mut expand = Vec::new();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let shown = show_nodes(ui, &nodes);
                            clicked = shown.clicked;
                            expand = shown.expand;
                        });
                        if !expand.is_empty() {
                            ui.ctx().request_repaint();
                        }
                        if let Some(tree) = &mut self.tree {
                            for path in expand {
                                match tree.expand(&path, &self.fs) {
                                    Ok(()) => {
                                        let n = tree
                                            .find(&path)
                                            .map(|node| node.children().len())
                                            .unwrap_or(0);
                                        self.run_log.log_tree_expand(&path, n, None);
                                    }
                                    Err(e) => {
                                        self.run_log.log_tree_expand(
                                            &path,
                                            0,
                                            Some(&e.to_string()),
                                        );
                                        self.status = e.to_string();
                                    }
                                }
                            }
                        }
                        if let Some(path) = clicked {
                            self.open_path(&path);
                        }
                    }
                    _ => {
                        ui.label("Open a folder or file.");
                    }
                }
            });
        self.layout
            .set_left_width(tree_response.response.rect.width());

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.tabs.is_empty() {
                    ui.weak("No tabs");
                } else {
                    let tabs: Vec<_> = self.tabs.tabs().to_vec();
                    for tab in &tabs {
                        let selected = self.tabs.focused() == Some(tab);
                        let dirty = self
                            .buffers
                            .get(tab.as_path())
                            .map(OpenBuffer::is_dirty)
                            .unwrap_or(false);
                        let label = if dirty {
                            format!("{}*", tab.label())
                        } else {
                            tab.label()
                        };
                        if ui.selectable_label(selected, label).clicked() {
                            self.tabs.focus(tab);
                        }
                        if ui.small_button("×").clicked() {
                            self.close_tab(tab);
                        }
                    }
                }
            });
            ui.separator();
            match self.tabs.focused().cloned() {
                Some(id) => {
                    if !self.buffers.contains(id.as_path()) {
                        self.open_path(id.as_path());
                    }
                    ui.label(id.as_path().display().to_string());
                    if let Some(buf) = self.buffers.get_mut(id.as_path()) {
                        let change = show_editor(ui, buf, &self.highlighter, &mut self.clipboard);
                        if let Some((path, old, new)) = change {
                            if let Some(lsp) = &mut self.lsp {
                                match lsp.did_change(&path, &old, &new) {
                                    Ok(()) => self.run_log.log_lsp("textDocument/didChange", None),
                                    Err(e) => {
                                        self.run_log.log_lsp(
                                            "textDocument/didChange",
                                            Some(&e.to_string()),
                                        );
                                        self.status = e.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    ui.label("No file open");
                }
            }
        });
    }
}

struct ShowTree {
    clicked: Option<PathBuf>,
    expand: Vec<PathBuf>,
}

fn show_nodes(ui: &mut egui::Ui, nodes: &[TreeNode]) -> ShowTree {
    let mut clicked = None;
    let mut expand = Vec::new();
    for node in nodes {
        if node.is_dir() {
            egui::CollapsingHeader::new(node.name())
                .default_open(true)
                .show(ui, |ui| {
                    if !node.is_loaded() {
                        expand.push(node.path().to_path_buf());
                    }
                    let inner = show_nodes(ui, node.children());
                    if clicked.is_none() {
                        clicked = inner.clicked;
                    }
                    expand.extend(inner.expand);
                });
        } else if ui.selectable_label(false, node.name()).clicked() {
            clicked = Some(node.path().to_path_buf());
        }
    }
    ShowTree { clicked, expand }
}

enum DiscoverKind {
    Definition,
    Implementation,
    References,
}

fn show_editor(
    ui: &mut egui::Ui,
    buffer: &mut OpenBuffer,
    highlighter: &Highlighter,
    clipboard: &mut impl ClipboardPort,
) -> Option<(PathBuf, String, String)> {
    let path = buffer.path().to_path_buf();
    let mut text = buffer.text();
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let s = buf.as_str();
        let spans = highlighter.highlight(&path, s);
        let job = layout_job_from_spans(s, &spans, wrap_width);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let output = egui::TextEdit::multiline(&mut text)
        .code_editor()
        .desired_width(f32::INFINITY)
        .desired_rows(24)
        .layouter(&mut layouter)
        .show(ui);
    if output.response.changed() && text != buffer.text() {
        let old = buffer.text();
        let path = buffer.path().to_path_buf();
        sync_buffer_from_view(buffer, &text, clipboard);
        Some((path, old, text))
    } else {
        None
    }
}

fn sync_buffer_from_view(
    buffer: &mut OpenBuffer,
    new_text: &str,
    clipboard: &mut impl ClipboardPort,
) {
    let len = buffer.len_chars();
    let _ = EditCommand::select(Selection::new(0, len)).apply(buffer, clipboard);
    if new_text.is_empty() {
        let _ = EditCommand::delete().apply(buffer, clipboard);
    } else {
        let _ = EditCommand::insert(new_text).apply(buffer, clipboard);
    }
}

fn layout_job_from_spans(text: &str, spans: &[HighlightSpan], wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let font = egui::FontId::monospace(14.0);
    if spans.is_empty() {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font,
                color: egui::Color32::from_rgb(200, 200, 200),
                ..Default::default()
            },
        );
        return job;
    }
    let mut cursor = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    for span in spans {
        let start = span.start().min(total);
        let end = span.end().min(total);
        if start > cursor {
            let prefix: String = chars[cursor..start].iter().collect();
            job.append(
                &prefix,
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: egui::Color32::from_rgb(200, 200, 200),
                    ..Default::default()
                },
            );
        }
        if end > start {
            let piece: String = chars[start..end].iter().collect();
            job.append(
                &piece,
                0.0,
                egui::TextFormat {
                    font_id: font.clone(),
                    color: egui::Color32::from_rgb(span.r(), span.g(), span.b()),
                    ..Default::default()
                },
            );
        }
        cursor = end.max(cursor);
    }
    if cursor < total {
        let suffix: String = chars[cursor..].iter().collect();
        job.append(
            &suffix,
            0.0,
            egui::TextFormat {
                font_id: font,
                color: egui::Color32::from_rgb(200, 200, 200),
                ..Default::default()
            },
        );
    }
    job
}
