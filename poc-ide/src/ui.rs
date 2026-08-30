//! eframe view. Ignored by llvm-cov. Domain state lives in the lib.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::text::LayoutJob;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use poc_ide::{
    advertised_control_socket, BufferMap, ClipboardPort, CompactChain, ConflictChoice,
    ControlClient, CursorOffsets, DialogPort, DiscoverKind, DiskEvent, DiskWatch, EditCommand,
    FileTree, FsPort, HighlightSpan, Highlighter, IdeError, LayoutState, LspClient, LspSessionState,
    NotifyWatch, DialogOutcome, OpenBuffer, PendingDialog, PendingDiscover, RunLog, Selection, ServeMode,
    SpawnSpec, StdFs, StdioLsp, TabId, TabStrip, TreeExpansion, TreeNode, UnixControl, WatchPort,
    WorkspaceRoot,
};
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
        if self.mapped.is_watching(path) {
            return Ok(());
        }
        if let Some(watcher) = &mut self.watcher {
            watcher
                .watch(path, RecursiveMode::NonRecursive)
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
    expansion: TreeExpansion,
    tabs: TabStrip,
    buffers: BufferMap,
    highlighter: Highlighter,
    layout: LayoutState,
    watch: LiveWatch,
    disk: DiskWatch,
    lsp: Option<LspClient<StdioLsp>>,
    lsp_inbox: Option<mpsc::Receiver<Result<LspClient<StdioLsp>, String>>>,
    lsp_session: LspSessionState,
    lsp_error: Option<String>,
    serve_mode: ServeMode,
    control_socket_path: Option<PathBuf>,
    /// Live Envelope connection when `ServeMode::ControlSocket`. Held so the
    /// socket stays up; the bin has no inspector that sends through it.
    #[allow(dead_code)]
    control: Option<ControlClient<UnixControl>>,
    control_error: Option<String>,
    status: String,
    run_log: RunLog,
    pending_discover: Option<PendingDiscover>,
    pending_dialog: Option<PendingDialog>,
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
        let mut app = Self {
            dialog: RfdDialog,
            fs: StdFs,
            clipboard: ArboardClipboard,
            root: None,
            tree: None,
            expansion: TreeExpansion::new(),
            tabs: TabStrip::new(),
            buffers: BufferMap::new(),
            highlighter: Highlighter::new(),
            layout: LayoutState::new(),
            watch: LiveWatch::new(),
            disk: DiskWatch::new(),
            lsp: None,
            lsp_inbox: None,
            lsp_session: LspSessionState::Idle,
            lsp_error: None,
            serve_mode,
            control_socket_path: control_socket,
            control: None,
            control_error: None,
            status: String::new(),
            run_log,
            pending_discover: None,
            pending_dialog: None,
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
                self.spawn_lsp(&root);
                self.expansion = TreeExpansion::for_root(&root);
                self.root = Some(root);
                self.tree = Some(tree);
                self.status = watch_err
                    .map(|e| e.to_string())
                    .unwrap_or_else(|| "Connecting language server…".into());
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

    fn queue_dialog(&mut self, pending: PendingDialog) {
        self.pending_dialog = Some(pending);
    }

    fn apply_pending_dialog(&mut self) {
        let Some(pending) = self.pending_dialog.take() else {
            return;
        };
        match pending.apply(&mut self.dialog, &self.fs) {
            Ok(DialogOutcome::Cancelled) => {}
            Ok(DialogOutcome::Folder(root)) => {
                self.run_log.log_open_folder(root.as_path());
                self.set_root(root, None);
            }
            Ok(DialogOutcome::File { root, path }) => {
                self.run_log.log_open_file(&path);
                self.set_root(root, Some(path));
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn open_path(&mut self, path: &Path) {
        match self.buffers.open(path, &self.fs) {
            Ok(buf) => {
                let opened = buf.path().to_path_buf();
                let text = buf.text();
                self.tabs.open(&opened);
                self.run_log.log_tab_open(&opened);
                if let Some(parent) = opened.parent() {
                    let _ = self.watch.watch(parent);
                }
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

    fn spawn_lsp(&mut self, root: &WorkspaceRoot) {
        self.shutdown_lsp();
        self.lsp_session = self.lsp_session.begin_connect();
        self.run_log.log_lsp("initialize_start", None);
        let (tx, rx) = mpsc::channel();
        self.lsp_inbox = Some(rx);
        let root = root.as_path().to_path_buf();
        let mode = self.serve_mode;
        let socket = self.control_socket_path.clone();
        let _ = std::thread::Builder::new()
            .name("poc-ide-lsp".into())
            .spawn(move || {
                let result = spawn_initialized_client(root, mode, socket.as_deref());
                let _ = tx.send(result.map_err(|e| e.to_string()));
            });
    }

    fn poll_lsp_inbox(&mut self) {
        let incoming = match &self.lsp_inbox {
            None => return,
            Some(rx) => match rx.try_recv() {
                Ok(v) => Some(v),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("language server thread ended".into()))
                }
            },
        };
        let Some(incoming) = incoming else {
            return;
        };
        self.lsp_inbox = None;
        match incoming {
            Ok(client) => self.finish_lsp(client),
            Err(e) => {
                self.run_log.log_lsp("initialize", Some(&e));
                self.lsp = None;
                self.lsp_error = Some(e.clone());
                self.lsp_session = self.lsp_session.finish_err();
                if self.serve_mode.is_control_socket() {
                    self.control_error = Some(e.clone());
                }
                self.status = e;
            }
        }
    }

    fn finish_lsp(&mut self, mut client: LspClient<StdioLsp>) {
        self.run_log.log_lsp("initialize", None);
        self.connect_control(client.progressive_cap());
        let opens: Vec<(PathBuf, String)> = self
            .tabs
            .tabs()
            .iter()
            .filter_map(|tab| {
                self.buffers
                    .get(tab.as_path())
                    .map(|buf| (buf.path().to_path_buf(), buf.text()))
            })
            .collect();
        for (path, text) in opens {
            match client.did_open(&path, &text) {
                Ok(_) => self.run_log.log_lsp("textDocument/didOpen", None),
                Err(e) => {
                    self.run_log
                        .log_lsp("textDocument/didOpen", Some(&e.to_string()));
                    self.status = e.to_string();
                }
            }
        }
        self.lsp = Some(client);
        self.lsp_error = None;
        self.lsp_session = self.lsp_session.finish_ok();
        if self.status == "Connecting language server…" {
            self.status.clear();
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
                }
            },
            Err(e) => {
                self.run_log.log_control_connect_error(&e.to_string());
                self.control_error = Some(e.to_string());
            }
        }
    }

    fn shutdown_lsp(&mut self) {
        self.lsp_inbox = None;
        self.control = None;
        self.lsp_session = LspSessionState::Idle;
        if let Some(mut client) = self.lsp.take() {
            let _ = client.shutdown();
        }
    }

    fn missing_server_status(&self) -> String {
        self.lsp_error
            .clone()
            .unwrap_or_else(|| IdeError::MissingBinary.to_string())
    }

    fn queue_discover(&mut self, kind: DiscoverKind) {
        self.pending_discover = Some(PendingDiscover::record(kind));
    }

    fn apply_pending_discover(&mut self) {
        let Some(pending) = self.pending_discover.take() else {
            return;
        };
        match pending.apply(
            self.lsp.as_mut(),
            &mut self.tabs,
            &mut self.buffers,
            &self.fs,
            Some(&mut self.run_log),
        ) {
            Ok(0) => self.status = "No locations".into(),
            Ok(_) => self.status.clear(),
            Err(e) if e.is_missing_binary() => self.status = self.missing_server_status(),
            Err(e) => self.status = e.to_string(),
        }
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
        self.apply_pending_dialog();
        self.poll_lsp_inbox();
        if self.lsp_inbox.is_some() {
            ui.ctx().request_repaint();
        }
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

        if ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command) {
            self.save_focused();
        }
        if ui.input(|i| i.key_pressed(egui::Key::F12) && i.modifiers.shift) {
            self.queue_discover(DiscoverKind::References);
        } else if ui.input(|i| i.key_pressed(egui::Key::F12) && i.modifiers.command) {
            self.queue_discover(DiscoverKind::Implementation);
        } else if ui.input(|i| i.key_pressed(egui::Key::F12)) {
            self.queue_discover(DiscoverKind::Definition);
        }

        egui::Panel::top("menu").resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Open Folder…").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_dialog(PendingDialog::open_folder());
                        ui.ctx().request_repaint();
                    }
                    if ui.button("Open File…").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_dialog(PendingDialog::open_file());
                        ui.ctx().request_repaint();
                    }
                    if ui.button("Save").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.save_focused();
                    }
                });
                ui.menu_button("Navigate", |ui| {
                    if ui.button("Go to Definition").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::Definition);
                    }
                    if ui.button("Go to Implementation").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::Implementation);
                    }
                    if ui.button("Find References").clicked() {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::References);
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
                if root_label.is_some() && self.tree.is_some() {
                    let label = root_label.unwrap();
                    ui.label(label);
                    ui.separator();
                    let mut clicked = None;
                    let mut discover = None;
                    let mut became_expanded = Vec::new();
                    let mut became_collapsed = Vec::new();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(tree) = self.tree.as_ref() {
                            let shown = show_nodes(ui, tree.children(), &self.expansion);
                            clicked = shown.clicked;
                            discover = shown.discover;
                            became_expanded = shown.became_expanded;
                            became_collapsed = shown.became_collapsed;
                        }
                    });
                    if !became_expanded.is_empty() || !became_collapsed.is_empty() {
                        ui.ctx().request_repaint();
                    }
                    for path in became_collapsed {
                        self.expansion.collapse(&path);
                    }
                    if let Some(tree) = &mut self.tree {
                        for path in became_expanded {
                            match tree.load_compact_chain(&path, &self.fs) {
                                Ok(()) => {
                                    let n = tree
                                        .find(&path)
                                        .map(|node| node.compact_tail().children().len())
                                        .unwrap_or(0);
                                    self.run_log.log_tree_expand(&path, n, None);
                                    let expand_row = tree
                                        .find(&path)
                                        .and_then(CompactChain::from_node)
                                        .map(|chain| chain.path() == path)
                                        .unwrap_or(true);
                                    if expand_row {
                                        if let Err(e) = self.expansion.expand(&path, tree) {
                                            self.status = e.to_string();
                                        }
                                    }
                                    let _ = self.watch.watch(&path);
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
                    if let Some(kind) = discover {
                        self.queue_discover(kind);
                    }
                } else {
                    ui.label("Open a folder or file.");
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
                        let outcome = show_editor(ui, buf, &self.highlighter, &mut self.clipboard);
                        if let Some((path, old, new)) = outcome.change {
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
                        if let Some(kind) = outcome.discover {
                            self.queue_discover(kind);
                        }
                    }
                }
                None => {
                    ui.label("No file open");
                }
            }
        });
        self.apply_pending_discover();
    }
}

struct ShowTree {
    clicked: Option<PathBuf>,
    discover: Option<DiscoverKind>,
    became_expanded: Vec<PathBuf>,
    became_collapsed: Vec<PathBuf>,
}

fn discover_context_menu(ui: &mut egui::Ui, chosen: &mut Option<DiscoverKind>) {
    if ui.button("Find Definition").clicked() {
        ui.close_kind(egui::UiKind::Menu);
        *chosen = Some(DiscoverKind::Definition);
    }
    if ui.button("Find Implementation").clicked() {
        ui.close_kind(egui::UiKind::Menu);
        *chosen = Some(DiscoverKind::Implementation);
    }
    if ui.button("Find References").clicked() {
        ui.close_kind(egui::UiKind::Menu);
        *chosen = Some(DiscoverKind::References);
    }
}

fn show_nodes(ui: &mut egui::Ui, nodes: &[TreeNode], expansion: &TreeExpansion) -> ShowTree {
    let mut clicked = None;
    let mut discover = None;
    let mut became_expanded = Vec::new();
    let mut became_collapsed = Vec::new();
    for node in nodes {
        if node.is_dir() {
            let Some(chain) = CompactChain::from_node(node) else {
                continue;
            };
            let path = chain.path();
            let open = expansion.is_expanded(path);
            let tail = node.compact_tail();
            let response = egui::CollapsingHeader::new(chain.display_name())
                .id_salt(path)
                .default_open(false)
                .open(Some(open))
                .show(ui, |ui| show_nodes(ui, tail.children(), expansion));
            if let Some(inner) = response.body_returned {
                if clicked.is_none() {
                    clicked = inner.clicked;
                }
                if discover.is_none() {
                    discover = inner.discover;
                }
                became_expanded.extend(inner.became_expanded);
                became_collapsed.extend(inner.became_collapsed);
            }
            if response.header_response.clicked() {
                if open {
                    became_collapsed.push(path.to_path_buf());
                } else {
                    became_expanded.push(path.to_path_buf());
                }
            }
        } else {
            let response = ui.selectable_label(false, node.name());
            if response.clicked() {
                clicked = Some(node.path().to_path_buf());
            }
            response.context_menu(|ui| discover_context_menu(ui, &mut discover));
        }
    }
    ShowTree {
        clicked,
        discover,
        became_expanded,
        became_collapsed,
    }
}

struct EditorOutcome {
    change: Option<(PathBuf, String, String)>,
    discover: Option<DiscoverKind>,
}

fn show_editor(
    ui: &mut egui::Ui,
    buffer: &mut OpenBuffer,
    highlighter: &Highlighter,
    clipboard: &mut impl ClipboardPort,
) -> EditorOutcome {
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
    let mut discover = None;
    output
        .response
        .context_menu(|ui| discover_context_menu(ui, &mut discover));
    let change = if output.response.changed() && text != buffer.text() {
        let old = buffer.text();
        let path = buffer.path().to_path_buf();
        sync_buffer_from_view(buffer, &text, clipboard);
        Some((path, old, text))
    } else {
        None
    };
    if let Some(range) = output
        .cursor_range
        .or_else(|| output.state.cursor.char_range())
    {
        let sorted = range.as_sorted_char_range();
        CursorOffsets::new(usize::from(sorted.start), usize::from(sorted.end)).apply(buffer);
    }
    EditorOutcome { change, discover }
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

fn spawn_initialized_client(
    root: PathBuf,
    mode: ServeMode,
    control_socket: Option<&Path>,
) -> Result<LspClient<StdioLsp>, IdeError> {
    let spec = SpawnSpec::resolve()?;
    let transport = StdioLsp::spawn_serve(&spec, mode, control_socket)?;
    let mut client = LspClient::new(transport).with_mode(mode);
    client.initialize(&root)?;
    Ok(client)
}
