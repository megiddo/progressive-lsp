//! eframe view. Ignored by llvm-cov. Domain state lives in the lib.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::text::LayoutJob;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use poc_ide::{
    advertised_control_socket, spawn_control_io, spawn_lsp_io, spawn_mux_control_io,
    spawn_runtime_io, spawn_tree_io, BufferMap, ClipboardPort, CompactChain, ConflictChoice,
    ControlIoEvent, ControlIoHandle, ControlPush, ControlPushInbox, ControlSocketPath,
    CursorOffsets, DialogAction, DialogOutcome, DialogPort, DiscoverFlight, DiscoverKind,
    DiscoverMenu, DiskEvent, DiskWatch, DockerRunPlan, EditCommand, FileTree, FsPort,
    HighlightSpan, Highlighter, HostOs, IdeError, LanguageCatalog, LaunchJournal, LayoutState,
    LspIoAttach, LspIoEvent, LspIoHandle, LspIoRequest, LspSessionState, NotifyWatch, OpenBuffer,
    OpenMode, PackageTierMap, PendingDialog, PendingDiscover, ProofStatus, RunLog, RuntimeIoEvent,
    RuntimeIoHandle, RuntimeIoRequest, Selection, ServeMode, ServeSpawn, ServeWalPath, SpawnSpec,
    StatusModal, StatusModalKind, StdFs, SystemClock, T3HostOffer, TabId, TabStrip, TierCellKind,
    TierStrip, TreeExpandFlight, TreeExpansion, TreeIoEvent, TreeIoHandle, TreeIoRequest, TreeNode,
    WatchPort, WireTier, WorkspaceRoot, PLAIN_TEXT_RGB,
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
    lsp_io: Option<LspIoHandle>,
    lsp_session: LspSessionState,
    lsp_error: Option<String>,
    serve_mode: ServeMode,
    serve_spawn: ServeSpawn,
    spawn_binary: Option<PathBuf>,
    control_io: Option<ControlIoHandle>,
    control_inbox: ControlPushInbox,
    control_error: Option<String>,
    catalog: LanguageCatalog,
    tiers: PackageTierMap,
    discover_flight: DiscoverFlight,
    tree_io: TreeIoHandle,
    tree_flight: TreeExpandFlight,
    status: String,
    run_log: RunLog,
    pending_discover: Option<PendingDiscover>,
    pending_dialog: Option<PendingDialog>,
    host: HostOs,
    open_mode: OpenMode,
    status_modal: StatusModal,
    launch_journal: LaunchJournal,
    runtime_io: RuntimeIoHandle,
}

impl PocIdeApp {
    pub fn new(
        folder: Option<PathBuf>,
        file: Option<PathBuf>,
        control_socket: Option<PathBuf>,
        run_log: RunLog,
        open_mode: OpenMode,
    ) -> Self {
        let socket = ControlSocketPath::resolve_default(control_socket.as_deref());
        let _ = socket.ensure_parent();
        let wal = ServeWalPath::resolve_default(&SystemClock);
        let _ = wal.ensure_parent();
        let serve_mode = ServeMode::ControlSocket;
        let serve_spawn = ServeSpawn::new(
            serve_mode,
            Some(socket.as_path()),
            Some(wal.as_path().to_path_buf()),
        )
        .expect("ControlSocket always owns a path");
        let spawn_binary = SpawnSpec::resolve().ok().map(|s| s.binary().to_path_buf());
        let mut run_log = run_log;
        let run_log_path = run_log.path().map(Path::to_path_buf);
        run_log.log_run_start(
            &serve_spawn.run_start(spawn_binary.as_deref(), run_log_path.as_deref()),
        );
        let host = HostOs::current();
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
            lsp_io: None,
            lsp_session: LspSessionState::Idle,
            lsp_error: None,
            serve_mode,
            serve_spawn,
            spawn_binary,
            control_io: None,
            control_inbox: ControlPushInbox::new(),
            control_error: None,
            catalog: LanguageCatalog::new(),
            tiers: PackageTierMap::new(),
            discover_flight: DiscoverFlight::idle(),
            tree_io: spawn_tree_io(),
            tree_flight: TreeExpandFlight::idle(),
            status: String::new(),
            run_log,
            pending_discover: None,
            pending_dialog: None,
            host,
            open_mode: open_mode.for_host(host),
            status_modal: StatusModal::closed(),
            launch_journal: LaunchJournal::new(),
            runtime_io: spawn_runtime_io(),
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
                self.run_log
                    .log_open_folder_mode(root.as_path(), self.open_mode);
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
                self.tree_flight = TreeExpandFlight::idle();
                self.expansion = TreeExpansion::for_root(&root);
                let container = self.open_mode == OpenMode::Container;
                if container {
                    self.launch_journal = LaunchJournal::container_plan();
                    self.status_modal = StatusModal::open(StatusModalKind::Container);
                    match self
                        .runtime_io
                        .submit(RuntimeIoRequest::launch(root.as_path()))
                    {
                        Ok(()) => {
                            self.status = "Starting Linux container host…".into();
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                } else {
                    if !self.open_mode.offers_t3(self.host) {
                        self.launch_journal = LaunchJournal::native_t3_skipped();
                    } else {
                        self.launch_journal = LaunchJournal::new();
                    }
                    self.spawn_lsp(&root);
                }
                self.root = Some(root);
                self.tree = Some(tree);
                if !container {
                    self.status = watch_err
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "Connecting language server…".into());
                } else if let Some(e) = watch_err {
                    self.status = e.to_string();
                }
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
                self.open_mode = match pending.action() {
                    DialogAction::OpenFolderInContainer => OpenMode::Container,
                    _ => OpenMode::Native,
                }
                .for_host(self.host);
                self.run_log
                    .log_open_folder_mode(root.as_path(), self.open_mode);
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
                if self.lsp_session.is_ready() {
                    if let Some(io) = &self.lsp_io {
                        match io.submit(LspIoRequest::DidOpen {
                            path: opened.clone(),
                            text,
                        }) {
                            Ok(()) => self.run_log.log_lsp("textDocument/didOpen", None),
                            Err(e) => {
                                self.run_log
                                    .log_lsp("textDocument/didOpen", Some(&e.to_string()));
                                self.status = e.to_string();
                                return;
                            }
                        }
                    }
                }
                self.status.clear();
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn close_tab(&mut self, id: &TabId) {
        if self.lsp_session.is_ready() {
            if let Some(io) = &self.lsp_io {
                let _ = io.submit(LspIoRequest::DidClose {
                    path: id.as_path().to_path_buf(),
                });
            }
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
                    if self.lsp_session.is_ready() {
                        if let Some(io) = &self.lsp_io {
                            match io.submit(LspIoRequest::DidSave {
                                path: id.as_path().to_path_buf(),
                            }) {
                                Ok(()) => self.run_log.log_lsp("textDocument/didSave", None),
                                Err(e) => {
                                    self.run_log
                                        .log_lsp("textDocument/didSave", Some(&e.to_string()));
                                    self.status = e.to_string();
                                    return;
                                }
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
        let attach = if self.open_mode == OpenMode::Container {
            self.serve_mode = ServeMode::Mux;
            match DockerRunPlan::new("docker", root.as_path()) {
                Ok(plan) => LspIoAttach::Container(plan),
                Err(e) => {
                    self.lsp_session = self.lsp_session.finish_err();
                    self.lsp_error = Some(e.to_string());
                    self.status = e.to_string();
                    return;
                }
            }
        } else {
            self.serve_mode = ServeMode::ControlSocket;
            LspIoAttach::Native(self.serve_spawn.clone())
        };
        self.lsp_io = Some(spawn_lsp_io(root.as_path().to_path_buf(), attach));
    }

    fn poll_tree_inbox(&mut self) {
        let events = self.tree_io.poll();
        for ev in events {
            self.handle_tree_event(ev);
        }
    }

    fn poll_runtime_inbox(&mut self) {
        let events = self.runtime_io.poll();
        for ev in events {
            self.handle_runtime_event(ev);
        }
    }

    fn handle_runtime_event(&mut self, ev: RuntimeIoEvent) {
        let finished = ev.is_finished();
        self.launch_journal = ev.journal().clone();
        if finished {
            self.run_log.log_container_journal(&self.launch_journal);
            if self.launch_journal.serve_ready() {
                if let Some(root) = self.root.clone() {
                    self.spawn_lsp(&root);
                    self.status = "Connecting language server…".into();
                }
            } else if self.launch_journal.is_failed() {
                self.status = "Container host failed — see launch log".into();
            }
        }
    }

    fn handle_tree_event(&mut self, ev: TreeIoEvent) {
        let path = ev.path().to_path_buf();
        self.tree_flight.finish(&path);
        match ev {
            TreeIoEvent::Expanded { listing } => {
                if let Some(tree) = &mut self.tree {
                    match tree.apply_listing(&listing) {
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
                            self.run_log.log_tree_expand(&path, 0, Some(&e.to_string()));
                            self.status = e.to_string();
                        }
                    }
                }
            }
            TreeIoEvent::Failed { error, .. } => {
                self.run_log.log_tree_expand(&path, 0, Some(&error));
                self.status = error;
            }
        }
    }

    fn queue_tree_expand(&mut self, path: PathBuf) {
        if !self.tree_flight.can_expand(&path) {
            return;
        }
        if let Some(tree) = &self.tree {
            if !tree.needs_compact_listing(&path) {
                self.run_log.log_tree_expand(
                    &path,
                    tree.find(&path)
                        .map(|node| node.compact_tail().children().len())
                        .unwrap_or(0),
                    None,
                );
                if let Some(tree) = &self.tree {
                    if let Err(e) = self.expansion.expand(&path, tree) {
                        self.status = e.to_string();
                    }
                }
                let _ = self.watch.watch(&path);
                return;
            }
        }
        self.tree_flight.begin(&path);
        if let Err(e) = self.tree_io.submit(TreeIoRequest::expand(&path)) {
            self.tree_flight.finish(&path);
            self.status = e.to_string();
        }
    }

    fn poll_lsp_inbox(&mut self) {
        let events = match &self.lsp_io {
            None => return,
            Some(io) => io.poll(),
        };
        for ev in events {
            self.handle_lsp_event(ev);
        }
    }

    fn handle_lsp_event(&mut self, ev: LspIoEvent) {
        match ev {
            LspIoEvent::Initialized { cap } => {
                self.run_log.log_lsp("initialize", None);
                self.lsp_error = None;
                self.lsp_session = self.lsp_session.finish_ok();
                self.connect_control(cap.as_ref());
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
                if let Some(io) = &self.lsp_io {
                    for (path, text) in opens {
                        match io.submit(LspIoRequest::DidOpen { path, text }) {
                            Ok(()) => self.run_log.log_lsp("textDocument/didOpen", None),
                            Err(e) => {
                                self.run_log
                                    .log_lsp("textDocument/didOpen", Some(&e.to_string()));
                                self.status = e.to_string();
                            }
                        }
                    }
                }
                if self.status == "Connecting language server…" {
                    self.status.clear();
                }
            }
            LspIoEvent::Opened { .. }
            | LspIoEvent::Changed { .. }
            | LspIoEvent::Saved { .. }
            | LspIoEvent::Closed { .. }
            | LspIoEvent::ShutdownDone => {}
            LspIoEvent::Discover {
                kind,
                path,
                line,
                character,
                locations,
            } => {
                self.discover_flight = self.discover_flight.finish();
                match locations {
                    Ok(locs) => match poc_ide::DiscoverCommand::new(kind).apply_locations(
                        &locs,
                        &path,
                        line,
                        character,
                        &mut self.tabs,
                        &mut self.buffers,
                        &self.fs,
                        Some(&mut self.run_log),
                        None,
                    ) {
                        Ok(0) => self.status = "No locations".into(),
                        Ok(_) => self.status.clear(),
                        Err(e) => self.status = e.to_string(),
                    },
                    Err(e) => {
                        let _ = poc_ide::DiscoverCommand::new(kind).apply_locations(
                            &[],
                            &path,
                            line,
                            character,
                            &mut self.tabs,
                            &mut self.buffers,
                            &self.fs,
                            Some(&mut self.run_log),
                            Some(&e),
                        );
                        self.status = if e.contains("binary") {
                            self.missing_server_status()
                        } else {
                            e
                        };
                    }
                }
            }
            LspIoEvent::Progress(p) => {
                self.run_log.log_progress(p.token(), p.kind().as_str());
            }
            LspIoEvent::LogMessage(m) => {
                self.run_log.log_window_log_message(m.typ(), m.message());
            }
            LspIoEvent::ChildStderr { line } => {
                self.run_log.log_child_stderr(&line);
            }
            LspIoEvent::Failed { method, error } => {
                self.run_log.log_lsp(&method, Some(&error));
                if method == "initialize" {
                    self.lsp_error = Some(error.clone());
                    self.lsp_session = self.lsp_session.finish_err();
                    if self.serve_mode.is_control_socket() {
                        self.control_error = Some(error.clone());
                    }
                }
                self.status = error;
            }
        }
    }

    fn connect_control(&mut self, cap: Option<&poc_ide::ProgressiveLspCap>) {
        self.control_io = None;
        self.control_error = None;
        if self.serve_mode.is_mux() {
            match self.lsp_io.as_ref().and_then(|h| h.take_mux_control()) {
                Some(ctl) => {
                    self.control_io = Some(spawn_mux_control_io(ctl));
                    self.control_error = None;
                }
                None => {
                    let err = IdeError::control("mux control missing");
                    self.run_log.log_control_connect_error(&err.to_string());
                    self.control_error = Some(err.to_string());
                }
            }
            return;
        }
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
            Ok(path) => {
                self.control_io = Some(spawn_control_io(path.to_string()));
                self.control_error = None;
            }
            Err(e) => {
                self.run_log.log_control_connect_error(&e.to_string());
                self.control_error = Some(e.to_string());
            }
        }
    }

    fn poll_control_inbox(&mut self) {
        let events = match &self.control_io {
            None => return,
            Some(io) => io.poll(),
        };
        for ev in events {
            match ev {
                ControlIoEvent::Connected => {}
                ControlIoEvent::IndexStatus(resp) => {
                    self.tiers.apply_index_status(&resp);
                }
                ControlIoEvent::TierStatus(resp) => {
                    self.tiers.apply_tier_status(&resp);
                }
                ControlIoEvent::Push(push) => {
                    self.run_log.log_control_push(push.method());
                    if let ControlPush::TierReady(ready) = &push {
                        self.tiers.apply_tier_ready(ready);
                    }
                    self.control_inbox.ingest(push);
                }
                ControlIoEvent::Failed(e) => {
                    self.run_log.log_control_connect_error(&e);
                    self.control_error = Some(e);
                }
            }
        }
    }

    fn shutdown_lsp(&mut self) {
        if let Some(io) = self.lsp_io.take() {
            let _ = io.submit(LspIoRequest::Shutdown);
        }
        self.control_io = None;
        self.control_inbox = ControlPushInbox::new();
        self.tiers = PackageTierMap::new();
        self.discover_flight = DiscoverFlight::idle();
        self.lsp_session = LspSessionState::Idle;
    }

    fn missing_server_status(&self) -> String {
        self.lsp_error
            .clone()
            .unwrap_or_else(|| IdeError::MissingBinary.to_string())
    }

    fn focused_language(&self) -> &str {
        match self.tabs.focused() {
            Some(id) => self.catalog.for_path(id.as_path()),
            None => "plaintext",
        }
    }

    fn focused_tier(&self) -> Option<WireTier> {
        match self.tabs.focused() {
            Some(id) => self.tiers.tier_for_path(id.as_path()),
            None => self.tiers.aggregate(),
        }
    }

    fn discover_menu(&self) -> DiscoverMenu {
        DiscoverMenu::paint_for_open(
            &self.catalog,
            self.focused_language(),
            self.lsp_session,
            &self.discover_flight,
            self.tiers.ingest_for_strip(self.lsp_session),
            self.focused_tier(),
            T3HostOffer::from_open(self.host, self.open_mode),
        )
    }

    fn tier_strip(&self) -> TierStrip {
        TierStrip::paint_for_open(
            &self.catalog,
            self.focused_language(),
            self.tiers.ingest_for_strip(self.lsp_session),
            self.focused_tier(),
            T3HostOffer::from_open(self.host, self.open_mode),
        )
    }

    fn journal_for_kind(&self, kind: StatusModalKind) -> LaunchJournal {
        match kind {
            StatusModalKind::T1 => {
                LaunchJournal::t1_from_ingest(self.tiers.ingest_for_strip(self.lsp_session))
            }
            StatusModalKind::T2 => {
                LaunchJournal::t2_from_ingest(self.tiers.ingest_for_strip(self.lsp_session))
            }
            StatusModalKind::Container => {
                if self.launch_journal.is_empty() {
                    LaunchJournal::container_plan()
                } else {
                    self.launch_journal.clone()
                }
            }
            StatusModalKind::T3 => {
                if !self.catalog.t3_supported(self.focused_language()) {
                    LaunchJournal::t3_not_supported()
                } else if !T3HostOffer::from_open(self.host, self.open_mode).is_offered() {
                    LaunchJournal::native_t3_skipped()
                } else {
                    LaunchJournal::native_t3_from_wire(
                        self.focused_tier(),
                        self.tiers.ingest_for_strip(self.lsp_session),
                    )
                }
            }
        }
    }

    fn queue_discover(&mut self, kind: DiscoverKind) {
        if !self.discover_menu().item(kind).can_submit() {
            return;
        }
        self.pending_discover = Some(PendingDiscover::record(kind));
    }

    fn apply_pending_discover(&mut self) {
        let Some(pending) = self.pending_discover.take() else {
            return;
        };
        if !self.discover_flight.can_submit() {
            return;
        }
        match pending.to_io_request(&self.tabs, &self.buffers) {
            Ok(req) => {
                if let Some(io) = &self.lsp_io {
                    match io.submit(req) {
                        Ok(()) => {
                            self.discover_flight = self.discover_flight.begin(pending.kind());
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                } else {
                    self.status = self.missing_server_status();
                }
            }
            Err(e) if e.is_missing_binary() => self.status = self.missing_server_status(),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn proof_status(&self) -> ProofStatus {
        let last = self
            .run_log
            .rows()
            .ok()
            .map(|rows| ProofStatus::last_discover_from_rows(&rows))
            .unwrap_or_default();
        ProofStatus::from_parts(
            self.spawn_binary.as_deref(),
            self.serve_spawn.log_level(),
            self.run_log.path(),
            self.serve_spawn.serve_wal_path(),
            last,
        )
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

    fn show_status_modal(&mut self, ui: &mut egui::Ui) {
        let Some(kind) = self.status_modal.kind() else {
            return;
        };
        let journal = self.journal_for_kind(kind);
        let title = match kind {
            StatusModalKind::Container => "Container launch".to_string(),
            _ => format!("{} status", kind.as_str()),
        };
        let mut close = false;
        egui::Modal::new(egui::Id::new("tier_status")).show(ui.ctx(), |ui| {
            ui.heading(title);
            ui.label(format!("Open mode: {}", self.open_mode.as_str()));
            ui.separator();
            for step in journal.steps() {
                let detail = step.detail().unwrap_or("");
                if detail.is_empty() {
                    ui.label(format!("[{}] {}", step.state().as_str(), step.label()));
                } else {
                    ui.label(format!(
                        "[{}] {}: {}",
                        step.state().as_str(),
                        step.label(),
                        detail
                    ));
                }
            }
            ui.separator();
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if close {
            self.status_modal.close();
        }
    }
}

impl eframe::App for PocIdeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.apply_pending_dialog();
        self.poll_lsp_inbox();
        self.poll_control_inbox();
        self.poll_tree_inbox();
        self.poll_runtime_inbox();
        if self.lsp_session.is_connecting()
            || self.discover_flight.is_in_flight()
            || !self.tree_flight.is_empty()
            || self.launch_journal.is_running()
            || self.status_modal.is_open() && self.open_mode == OpenMode::Container
        {
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
        self.show_status_modal(ui);

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
                    if self.host.shows_container_open() {
                        if ui.button(OpenMode::Container.folder_label()).clicked() {
                            ui.close_kind(egui::UiKind::Menu);
                            self.queue_dialog(PendingDialog::open_folder_in_container());
                            ui.ctx().request_repaint();
                        }
                        if ui.button(OpenMode::Native.folder_label()).clicked() {
                            ui.close_kind(egui::UiKind::Menu);
                            self.queue_dialog(PendingDialog::open_folder());
                            ui.ctx().request_repaint();
                        }
                    } else if ui.button(OpenMode::Native.folder_label()).clicked() {
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
                    let menu = self.discover_menu();
                    if ui
                        .add_enabled(
                            menu.item(DiscoverKind::Definition).can_submit(),
                            egui::Button::new(
                                menu.item(DiscoverKind::Definition)
                                    .label("Go to Definition"),
                            ),
                        )
                        .clicked()
                    {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::Definition);
                    }
                    if ui
                        .add_enabled(
                            menu.item(DiscoverKind::Implementation).can_submit(),
                            egui::Button::new(
                                menu.item(DiscoverKind::Implementation)
                                    .label("Go to Implementation"),
                            ),
                        )
                        .clicked()
                    {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::Implementation);
                    }
                    if ui
                        .add_enabled(
                            menu.item(DiscoverKind::References).can_submit(),
                            egui::Button::new(
                                menu.item(DiscoverKind::References).label("Find References"),
                            ),
                        )
                        .clicked()
                    {
                        ui.close_kind(egui::UiKind::Menu);
                        self.queue_discover(DiscoverKind::References);
                    }
                });
                if !self.status.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(200, 80, 80), &self.status);
                }
            });
        });

        egui::Panel::top("tier_strip")
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for cell in self.tier_strip().cells() {
                        let kind = match cell.kind() {
                            TierCellKind::T1 => StatusModalKind::T1,
                            TierCellKind::T2 => StatusModalKind::T2,
                            TierCellKind::T3 => StatusModalKind::T3,
                        };
                        if ui
                            .button(format!("{}: {}", cell.label(), cell.status()))
                            .clicked()
                        {
                            self.status_modal = StatusModal::open(kind);
                        }
                        ui.separator();
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
                    let menu = self.discover_menu();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        if let Some(tree) = self.tree.as_ref() {
                            let shown = show_nodes(
                                ui,
                                tree.children(),
                                &self.expansion,
                                &self.tree_flight,
                                menu,
                            );
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
                    for path in became_expanded {
                        self.queue_tree_expand(path);
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

        egui::Panel::bottom("proof")
            .resizable(false)
            .show(ui, |ui| {
                ui.small(self.proof_status().footer_line());
            });

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
                    let editor_menu = self.discover_menu();
                    if let Some(buf) = self.buffers.get_mut(id.as_path()) {
                        let outcome = show_editor(
                            ui,
                            buf,
                            &mut self.highlighter,
                            &mut self.clipboard,
                            editor_menu,
                        );
                        if let Some((path, old, new)) = outcome.change {
                            if self.lsp_session.is_ready() {
                                if let Some(io) = &self.lsp_io {
                                    match io.submit(LspIoRequest::DidChange {
                                        path,
                                        old_text: old,
                                        new_text: new,
                                    }) {
                                        Ok(()) => {
                                            self.run_log.log_lsp("textDocument/didChange", None)
                                        }
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

fn discover_context_menu(ui: &mut egui::Ui, chosen: &mut Option<DiscoverKind>, menu: DiscoverMenu) {
    let items = [
        (DiscoverKind::Definition, "Find Definition"),
        (DiscoverKind::Implementation, "Find Implementation"),
        (DiscoverKind::References, "Find References"),
    ];
    for (kind, enabled_label) in items {
        let item = menu.item(kind);
        if ui
            .add_enabled(
                item.can_submit(),
                egui::Button::new(item.label(enabled_label)),
            )
            .clicked()
        {
            ui.close_kind(egui::UiKind::Menu);
            *chosen = Some(kind);
        }
    }
}

fn show_nodes(
    ui: &mut egui::Ui,
    nodes: &[TreeNode],
    expansion: &TreeExpansion,
    flight: &TreeExpandFlight,
    menu: DiscoverMenu,
) -> ShowTree {
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
            let loading = flight.is_loading(path);
            let open = expansion.is_expanded(path) || loading;
            let tail = node.compact_tail();
            let response = egui::CollapsingHeader::new(chain.display_name())
                .id_salt(path)
                .default_open(false)
                .open(Some(open))
                .show(ui, |ui| {
                    if loading {
                        ui.label(TreeExpandFlight::loading_label());
                        ShowTree {
                            clicked: None,
                            discover: None,
                            became_expanded: Vec::new(),
                            became_collapsed: Vec::new(),
                        }
                    } else {
                        show_nodes(ui, tail.children(), expansion, flight, menu)
                    }
                });
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
            if response.header_response.clicked() && !loading {
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
            response.context_menu(|ui| discover_context_menu(ui, &mut discover, menu));
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
    highlighter: &mut Highlighter,
    clipboard: &mut impl ClipboardPort,
    menu: DiscoverMenu,
) -> EditorOutcome {
    let path = buffer.path().to_path_buf();
    let generation = buffer.generation();
    let mut text = buffer.text();
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let s = buf.as_str();
        let spans = highlighter.highlight(&path, s, generation);
        let job = layout_job_from_spans(s, &spans, wrap_width);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let output = egui::ScrollArea::both()
        .id_salt("editor-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let rows = text.lines().count().max(1);
            egui::TextEdit::multiline(&mut text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(rows)
                .layouter(&mut layouter)
                .show(ui)
        })
        .inner;
    let mut discover = None;
    output
        .response
        .context_menu(|ui| discover_context_menu(ui, &mut discover, menu));
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

fn layout_job_from_spans(text: &str, spans: &[HighlightSpan], _wrap_width: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    let font = egui::FontId::monospace(14.0);
    if spans.is_empty() {
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font,
                color: egui::Color32::from_rgb(
                    PLAIN_TEXT_RGB.0,
                    PLAIN_TEXT_RGB.1,
                    PLAIN_TEXT_RGB.2,
                ),
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
                    color: egui::Color32::from_rgb(
                        PLAIN_TEXT_RGB.0,
                        PLAIN_TEXT_RGB.1,
                        PLAIN_TEXT_RGB.2,
                    ),
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
                color: egui::Color32::from_rgb(
                    PLAIN_TEXT_RGB.0,
                    PLAIN_TEXT_RGB.1,
                    PLAIN_TEXT_RGB.2,
                ),
                ..Default::default()
            },
        );
    }
    job
}
