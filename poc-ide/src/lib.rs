//! POC IDE domain: Ports, file-tree Composite, tabs, layout, buffers, syntect,
//! disk-conflict Observer, language catalog, stock LSP client, `DiscoverCommand`,
//! LSP/control IO Command+Event mailbox, control Adapter, protocol console, and
//! per-run sqlite [`RunLog`] Repository.
//! The lib does not import `egui`, `eframe`, `egui_dock`, or `rfd`. Those stay
//! in the composition-root bin (`main.rs` / `ui.rs`).

pub mod buffer;
pub mod child_stderr;
pub mod conflict;
pub mod console;
pub mod control;
pub mod discover;
pub mod edit;
pub mod error;
pub mod highlight;
pub mod language;
pub mod layout;
pub mod log;
pub mod lsp;
pub mod lsp_io;
pub mod mux;
pub mod open_mode;
pub mod ports;
pub mod proof;
pub mod runtime;
pub mod runtime_io;
pub mod tabs;
pub mod tier;
pub mod tree;
pub mod tree_io;
pub mod watch;

pub use buffer::{BufferMap, CursorOffsets, DirtyFlag, OpenBuffer, Selection};
pub use child_stderr::{ChildStderrDrain, STDERR_DRAIN_CAP};
pub use conflict::{ConflictChoice, ConflictModal};
pub use console::{ProtocolConsole, TranscriptEntry, TranscriptKind, STOCK_LSP_METHODS};
pub use control::{
    advertised_control, advertised_control_socket, pump_control_io, request_control_status,
    spawn_control_io, spawn_mux_control_io, ControlAttach, ControlClient, ControlIoEvent,
    ControlIoHandle, ControlPush, ControlPushInbox, UnixControl, CONTROL_UNARY_METHODS,
};
pub use discover::{DiscoverCommand, DiscoverKind, PendingDiscover};
pub use edit::EditCommand;
pub use error::IdeError;
pub use highlight::{HighlightCache, HighlightKey, HighlightSpan, Highlighter, PLAIN_TEXT_RGB};
pub use language::{ControlSocketPath, DiscoverOffer, LanguageCatalog, ServeMode, WireTier};
pub use layout::LayoutState;
pub use log::{
    default_run_log_dir, run_log_dir, sanitize_payload, LogCategory, LogRow, RunLog, RunLogPath,
    RunStart, ServeWalPath, CHILD_LOG_LEVEL, EVENT_CHILD_STDERR, EVENT_CONFLICT_ENQUEUE,
    EVENT_CONFLICT_RESOLVE, EVENT_CONTAINER_STEP, EVENT_CONTROL_CONNECT_ERROR, EVENT_CONTROL_PUSH,
    EVENT_LOG_MESSAGE, EVENT_OPEN_FILE, EVENT_OPEN_FOLDER, EVENT_PROGRESS, EVENT_RUN_START,
    EVENT_SAVE, EVENT_TAB_CLOSE, EVENT_TAB_OPEN, EVENT_TREE_EXPAND, EVENT_TREE_LOAD,
    SERVE_WAL_NOT_OPEN,
};
pub use lsp::{
    build_serve_command, file_uri, path_from_file_uri, position_at, LspClient, LspLocation,
    LspSessionState, ProgressiveLspCap, ServeSpawn, SpawnSpec, StdioLsp,
};
pub use lsp_io::{
    classify_notification, dispatch_lsp_io, notifications_to_events, pump_lsp_io, run_lsp_io_ready,
    spawn_lsp_io, DiscoverFlight, LogMessageEvent, LspIoAttach, LspIoEvent, LspIoHandle,
    LspIoMailbox, LspIoRequest, LspProgressKind, ProgressEvent,
};
pub use mux::{MuxControl, MuxLsp, MuxStdio};
pub use open_mode::{parse_launch_args, HostOs, LaunchFlags, OpenMode, T3HostOffer};
pub use ports::{
    ClipboardPort, ClockPort, ControlTransport, DialogPort, DiskEvent, DiskEventKind,
    FakeClipboard, FakeClock, FakeControl, FakeDialog, FakeLsp, FakeWatch, FsPort, LspCall,
    LspTransport, MemFs, StdFs, SystemClock, WatchPort,
};
pub use proof::ProofStatus;
pub use runtime::{
    run_launch, run_launch_reporting, DockerRunPlan, DockerRuntime, FakeRuntime, LaunchJournal,
    LaunchStep, RuntimeInfo, RuntimePort, RuntimeSession, StatusModal, StatusModalKind, StepState,
    RUNTIME_IMAGE,
};
pub use runtime_io::{
    dispatch_runtime_io, pump_runtime_io, spawn_runtime_io, RuntimeIoEvent, RuntimeIoHandle,
    RuntimeIoMailbox, RuntimeIoRequest,
};
pub use tabs::{TabId, TabStrip};
pub use tier::{
    DiscoverMenu, DiscoverMenuItem, MenuDisableReason, PackageTierMap, TierCell, TierCellKind,
    TierCellState, TierStrip,
};
pub use tree::{
    CompactChain, CompactChainListing, DialogAction, DialogOutcome, ExpandChainCommand, FileTree,
    PendingDialog, TreeExpansion, TreeNode, WorkspaceRoot,
};
pub use tree_io::{
    dispatch_tree_io, pump_tree_io, spawn_tree_io, TreeExpandFlight, TreeIoEvent, TreeIoHandle,
    TreeIoMailbox, TreeIoRequest,
};
pub use watch::{DiskWatch, NotifyWatch, WatchDepth};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_reexports_resolve() {
        let _ = FakeDialog::new();
        let _ = FakeClipboard::new();
        let _ = MemFs::new();
        let _ = StdFs;
        let _ = TabStrip::new();
        let _ = LayoutState::new();
        let _ = BufferMap::new();
        let _ = DirtyFlag::clean();
        let _ = Selection::collapsed(0);
        let _ = CursorOffsets::collapsed(0);
        let _ = EditCommand::delete();
        let _ = DiscoverCommand::definition();
        let _ = DiscoverKind::Implementation;
        let _ = PendingDiscover::record(DiscoverKind::Definition);
        let _ = DiscoverFlight::idle();
        let _ = LspIoMailbox::new();
        let _ = LspIoRequest::Shutdown;
        let _ = LspIoEvent::ShutdownDone;
        let _ = ProgressEvent::new("t", LspProgressKind::Begin, None, None);
        let _ = LogMessageEvent::new(3, "hi");
        let _ = ControlPushInbox::new();
        let _ = ControlIoEvent::Connected;
        let _ = PendingDialog::open_folder();
        let _ = PendingDialog::open_folder_in_container();
        let _ = DialogAction::OpenFile;
        let _ = DialogAction::OpenFolderInContainer;
        let _ = DialogOutcome::Cancelled;
        let _ = Highlighter::new();
        let _ = HighlightSpan::new(0, 1, 0, 0, 0);
        let _ = HighlightKey::new("/ws/a.rs", 0);
        let _ = HighlightCache::new();
        let _ = ExpandChainCommand::new("/ws/a");
        let _ = CompactChainListing::empty("/ws");
        let _ = TreeExpandFlight::idle();
        let _ = TreeIoMailbox::new();
        let _ = TreeIoRequest::expand("/ws/a");
        let _ = TreeIoEvent::Failed {
            path: "/ws/a".into(),
            error: "x".into(),
        };
        let _ = TreeIoHandle::pair;
        let _ = FakeWatch::new();
        let _ = WatchDepth::Immediate;
        let _ = LspSessionState::Idle;
        let _ = FakeClock::at_unix_ms(1);
        let _ = SystemClock;
        let _ = DiskWatch::new();
        let _ = NotifyWatch::new();
        let _ = ConflictModal::new("/ws/a.rs", 1);
        let _ = ConflictChoice::LoadDisk;
        let _ = ConflictChoice::KeepMemory;
        let _ = DiskEvent::modify("/ws/a.rs", 1);
        let _ = DiskEventKind::Modify;
        let _ = LanguageCatalog::new();
        let _ = DiscoverOffer::new(DiscoverKind::Definition, WireTier::Syntax, WireTier::Graph);
        let _ = WireTier::Syntax;
        let _ = TierStrip::paint(
            &LanguageCatalog::new(),
            "java",
            progressive_lsp_control::IngestState::Done,
            Some(WireTier::Graph),
        );
        let _ = TierCell::new(TierCellKind::T1, TierCellState::Done);
        let _ = PackageTierMap::new();
        let _ = DiscoverMenu::paint(
            &LanguageCatalog::new(),
            "java",
            LspSessionState::Ready,
            &DiscoverFlight::idle(),
            progressive_lsp_control::IngestState::Done,
            Some(WireTier::Syntax),
        );
        let _ = MenuDisableReason::Connecting;
        let _ = MenuDisableReason::NeedsContainer;
        let _ = OpenMode::Native;
        let _ = HostOs::Other;
        let _ = T3HostOffer::NeedsContainer;
        let _ = LaunchFlags::default();
        let _ = parse_launch_args(std::iter::empty());
        let _ = FakeRuntime::ready();
        let _ = LaunchJournal::container_plan();
        let _ = LaunchStep::new("id", "label");
        let _ = StepState::Pending;
        let _ = StatusModal::closed();
        let _ = StatusModalKind::T3;
        let _ = StatusModalKind::Container;
        let _ = RuntimeIoMailbox::new();
        let _ = RuntimeIoRequest::launch("/ws");
        let _ = spawn_runtime_io;
        let _ = RUNTIME_IMAGE;
        let _ = DockerRunPlan::new("docker", "/ws");
        let _ = LspIoAttach::Native(ServeSpawn::new(ServeMode::StockStdio, None, None).unwrap());
        let _ = ServeMode::StockStdio;
        let _ = ServeMode::ControlSocket;
        let _ = ServeMode::Mux;
        let _ = ServeMode::default();
        let _ = ControlAttach::Mux;
        let _ = MuxStdio::from_pair(Vec::<u8>::new(), std::io::Cursor::new(Vec::<u8>::new()));
        let _ = ControlSocketPath::from_path("/tmp/poc-ide.sock");
        let _ = ServeWalPath::new("/tmp", 1, 1);
        let _ = ServeSpawn::new(ServeMode::StockStdio, None, None);
        let _ = ProofStatus::default();
        let _ = ChildStderrDrain::new();
        let _ = RunStart::bootstrap(None);
        let _ = build_serve_command(
            &SpawnSpec::from_path("/opt/progressive-lsp"),
            &ServeSpawn::new(ServeMode::StockStdio, None, None).unwrap(),
        );
        let _ = FakeLsp::new();
        let _ = LspCall::request("initialize", serde_json::json!({}));
        let _ = LspLocation::new("file:///ws/a.rs", 0, 0, 0, 0);
        let _ = SpawnSpec::from_path("/opt/progressive-lsp");
        let _ = position_at("fn x", 0);
        let _ = FakeControl::new();
        let _ = FakeControl::missing_socket();
        let _ = ControlClient::new(FakeControl::new());
        let _ = ProtocolConsole::new();
        let _ = TranscriptEntry::new(TranscriptKind::ControlPush, "WatchBatch", 0, "");
        let _ = TranscriptKind::ControlPush;
        let _ = STOCK_LSP_METHODS;
        let _ = CONTROL_UNARY_METHODS;
        assert!(FileTree::skips_display_name(".git"));
        assert!(TreeExpansion::new().is_empty());
        let leaf = TreeNode::File {
            name: "x.rs".into(),
            path: "/ws/x.rs".into(),
        };
        assert!(CompactChain::from_node(&leaf).is_none());
        assert_eq!(leaf.compact_tail().name(), "x.rs");
        assert!(IdeError::NotAbsolute(std::path::PathBuf::from("rel"))
            .to_string()
            .contains("absolute"));
        assert!(IdeError::watch("x").is_watch());
        assert!(IdeError::MissingBinary.is_missing_binary());
        assert!(IdeError::NoFileOpen.is_no_file_open());
        assert_eq!(
            DiscoverKind::References.lsp_method(),
            "textDocument/references"
        );
        assert_eq!(
            PendingDiscover::record(DiscoverKind::Definition).kind(),
            DiscoverKind::Definition
        );
        assert_eq!(
            CursorOffsets::new(3, 1).to_selection(),
            Selection::new(1, 3)
        );
        assert!(IdeError::control("x").is_control());
        assert!(IdeError::control_socket_missing().is_control_socket_missing());
        assert!(IdeError::pending_mux().is_pending_mux());
        assert!(LanguageCatalog::new().skips_did_open("/ws/a.txt"));
        assert!(!ServeMode::StockStdio.is_control_socket());
        assert!(ServeMode::ControlSocket.is_control_socket());
        assert!(ServeMode::Mux.is_mux());
        assert_eq!(ServeMode::default(), ServeMode::ControlSocket);
        assert_eq!(STDERR_DRAIN_CAP, 1024);
        assert_eq!(CHILD_LOG_LEVEL, "debug");
        assert_eq!(EVENT_CHILD_STDERR, "child_stderr");
        assert_eq!(SERVE_WAL_NOT_OPEN, "not open yet");
        assert!(ProtocolConsole::new().is_empty());
        assert!(!STOCK_LSP_METHODS.is_empty());
        assert_eq!(CONTROL_UNARY_METHODS.len(), 9);
        let _ = RunLog::unavailable(FakeClock::at_unix_ms(1));
        let _ = LogCategory::Run;
        let _ = LogRow::new(0, LogCategory::Run, EVENT_RUN_START, None);
        let _ = RunLogPath::new("/tmp/logs", 1, 1);
        let _ = run_log_dir(Some("/injected"), None);
        let _ = default_run_log_dir();
        let _ = sanitize_payload(None);
        assert_eq!(EVENT_OPEN_FOLDER, "open_folder");
        assert_eq!(EVENT_CONTAINER_STEP, "container_step");
        assert_eq!(EVENT_OPEN_FILE, "open_file");
        assert_eq!(EVENT_TREE_LOAD, "tree_load");
        assert_eq!(EVENT_TREE_EXPAND, "tree_expand");
        assert_eq!(EVENT_TAB_OPEN, "tab_open");
        assert_eq!(EVENT_TAB_CLOSE, "tab_close");
        assert_eq!(EVENT_SAVE, "save");
        assert_eq!(EVENT_CONTROL_CONNECT_ERROR, "control_connect_error");
        assert_eq!(EVENT_CONTROL_PUSH, "control_push");
        assert_eq!(EVENT_PROGRESS, "$/progress");
        assert_eq!(EVENT_LOG_MESSAGE, "window/logMessage");
        assert!(DiscoverFlight::idle().can_submit());
        assert!(TreeExpandFlight::idle().can_expand(std::path::Path::new("/ws")));
        assert_eq!(TreeExpandFlight::loading_label(), "loading…");
        assert!(TreeIoRequest::expand("/ws/a").is_expand());
        assert_eq!(
            LspProgressKind::parse("begin"),
            Some(LspProgressKind::Begin)
        );
        assert!(classify_notification(&serde_json::json!({
            "method": "$/progress",
            "params": { "token": "t", "value": { "kind": "end" } }
        }))
        .is_some());
        assert!(notifications_to_events(&[]).is_empty());
        let _ = pump_lsp_io::<crate::ports::FakeLsp>;
        let _ = dispatch_lsp_io::<crate::ports::FakeLsp>;
        let _ = run_lsp_io_ready::<crate::ports::FakeLsp>;
        let _ = spawn_lsp_io;
        let _ = pump_tree_io::<crate::ports::MemFs>;
        let _ = dispatch_tree_io::<crate::ports::MemFs>;
        let _ = spawn_tree_io;
        let _ = pump_control_io::<crate::ports::FakeControl>;
        let _ = request_control_status::<crate::ports::FakeControl>;
        let _ = spawn_control_io;
        let _ = LspIoHandle::pair;
        let _ = ControlIoHandle::pair;
        assert_eq!(EVENT_CONFLICT_ENQUEUE, "conflict_enqueue");
        assert_eq!(EVENT_CONFLICT_RESOLVE, "conflict_resolve");
        assert!(IdeError::log("x").is_log());
        assert!(IdeError::runtime("x").is_runtime());
    }
}
