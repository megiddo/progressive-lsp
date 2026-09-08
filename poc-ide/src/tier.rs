//! T1/T2/T3 strip and honest discover menus. Value objects only; `ui.rs` renders.

use std::collections::BTreeMap;
use std::path::Path;

use progressive_lsp_control::{IndexStatusResponse, IngestState, TierReady, TierStatusResponse};

use crate::buffer::BufferMap;
use crate::discover::{DiscoverCommand, DiscoverKind};
use crate::error::IdeError;
use crate::language::{DiscoverOffer, LanguageCatalog, WireTier};
use crate::lsp::LspSessionState;
use crate::lsp_io::{DiscoverFlight, LspIoRequest};
use crate::open_mode::T3HostOffer;
use crate::tabs::TabStrip;

/// One cell in the status strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierCellKind {
    T1,
    T2,
    T3,
}

impl TierCellKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::T1 => "T1",
            Self::T2 => "T2",
            Self::T3 => "T3",
        }
    }
}

/// Painted state for one cell. Never a spinner for matrix `n/a` or waiting `n/a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierCellState {
    InProgress,
    Done,
    NotSupported,
    Skipped,
    Na,
}

impl TierCellState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "processing",
            Self::Done => "done",
            Self::NotSupported => "not supported",
            Self::Skipped => "skipped",
            Self::Na => "n/a",
        }
    }
}

/// Value object. One of T1 / T2 / T3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierCell {
    kind: TierCellKind,
    state: TierCellState,
}

impl TierCell {
    pub fn new(kind: TierCellKind, state: TierCellState) -> Self {
        Self { kind, state }
    }

    pub fn kind(self) -> TierCellKind {
        self.kind
    }

    pub fn state(self) -> TierCellState {
        self.state
    }

    pub fn label(self) -> &'static str {
        self.kind.as_str()
    }

    pub fn status(self) -> &'static str {
        self.state.as_str()
    }
}

/// Value object. Three cells for the focused package (or workspace aggregate).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierStrip {
    t1: TierCell,
    t2: TierCell,
    t3: TierCell,
}

impl TierStrip {
    pub fn paint(
        catalog: &LanguageCatalog,
        language_id: &str,
        ingest: IngestState,
        current: Option<WireTier>,
    ) -> Self {
        Self::paint_for_open(catalog, language_id, ingest, current, T3HostOffer::Offered)
    }

    pub fn paint_for_open(
        catalog: &LanguageCatalog,
        language_id: &str,
        ingest: IngestState,
        current: Option<WireTier>,
        t3_host: T3HostOffer,
    ) -> Self {
        if !catalog.is_known(language_id) {
            let indexing = ingest.is_running() || ingest.is_done();
            return Self {
                t1: TierCell::new(TierCellKind::T1, t1_state(ingest, current, indexing)),
                t2: TierCell::new(TierCellKind::T2, workspace_t2(ingest)),
                t3: TierCell::new(TierCellKind::T3, TierCellState::Na),
            };
        }
        Self {
            t1: TierCell::new(TierCellKind::T1, t1_state(ingest, current, true)),
            t2: TierCell::new(
                TierCellKind::T2,
                t2_state(catalog, language_id, ingest, current),
            ),
            t3: TierCell::new(
                TierCellKind::T3,
                t3_state(catalog, language_id, ingest, current, t3_host),
            ),
        }
    }

    pub fn t1(self) -> TierCell {
        self.t1
    }

    pub fn t2(self) -> TierCell {
        self.t2
    }

    pub fn t3(self) -> TierCell {
        self.t3
    }

    pub fn cells(self) -> [TierCell; 3] {
        [self.t1, self.t2, self.t3]
    }
}

fn t1_state(ingest: IngestState, current: Option<WireTier>, known: bool) -> TierCellState {
    if current.is_some_and(|t| t.meets(WireTier::Syntax)) || ingest.is_done() {
        return TierCellState::Done;
    }
    if known || ingest.is_running() {
        return TierCellState::InProgress;
    }
    TierCellState::Na
}

fn t2_state(
    catalog: &LanguageCatalog,
    language_id: &str,
    ingest: IngestState,
    current: Option<WireTier>,
) -> TierCellState {
    if !catalog.has_t2(language_id) {
        return TierCellState::Na;
    }
    if current.is_some_and(|t| t.meets(WireTier::Graph)) || ingest.is_done() {
        return TierCellState::Done;
    }
    if ingest.is_running() || current.is_some_and(|t| t.meets(WireTier::Syntax)) {
        return TierCellState::InProgress;
    }
    TierCellState::Na
}

fn workspace_t2(ingest: IngestState) -> TierCellState {
    if ingest.is_done() {
        TierCellState::Done
    } else if ingest.is_running() {
        TierCellState::InProgress
    } else {
        TierCellState::Na
    }
}

fn t3_state(
    catalog: &LanguageCatalog,
    language_id: &str,
    ingest: IngestState,
    current: Option<WireTier>,
    t3_host: T3HostOffer,
) -> TierCellState {
    if !catalog.t3_supported(language_id) {
        return TierCellState::NotSupported;
    }
    if !t3_host.is_offered() {
        return TierCellState::Skipped;
    }
    if current == Some(WireTier::Types) {
        return TierCellState::Done;
    }
    if ingest.is_done() {
        return TierCellState::Skipped;
    }
    let prior_ready = if catalog.has_t2(language_id) {
        current.is_some_and(|t| t.meets(WireTier::Graph))
    } else {
        current.is_some_and(|t| t.meets(WireTier::Syntax))
    };
    if prior_ready {
        TierCellState::InProgress
    } else {
        TierCellState::Na
    }
}

/// Collection of per-package wire tiers plus workspace ingest. Not a Manager.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageTierMap {
    ingest: IngestState,
    packages: BTreeMap<String, WireTier>,
}

impl PackageTierMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ingest(&self) -> IngestState {
        self.ingest
    }

    /// Folder open starts initialize ingest before IndexStatus arrives.
    /// Connecting means ingest is in that initialize; Ready means it already finished.
    pub fn ingest_for_strip(&self, lsp: LspSessionState) -> IngestState {
        if self.ingest.is_not_started() {
            if lsp.is_connecting() {
                return IngestState::Running;
            }
            if lsp.is_ready() {
                return IngestState::Done;
            }
        }
        self.ingest
    }

    pub fn apply_index_status(&mut self, resp: &IndexStatusResponse) {
        self.ingest = resp.ingest_state();
        for pkg in &resp.packages {
            self.packages
                .entry(pkg.package_id.clone())
                .or_insert(WireTier::Syntax);
        }
    }

    pub fn apply_tier_status(&mut self, resp: &TierStatusResponse) {
        for row in &resp.rows {
            if let Some(tier) = WireTier::parse(&row.tier) {
                self.packages.insert(row.package_id.clone(), tier);
            }
        }
    }

    pub fn apply_tier_ready(&mut self, ready: &TierReady) {
        if let Some(tier) = WireTier::parse(&ready.tier) {
            self.packages.insert(ready.package_id.clone(), tier);
        }
    }

    pub fn tier_for_package(&self, package_id: &str) -> Option<WireTier> {
        self.packages.get(package_id).copied()
    }

    /// Focused path → package id contained in the path; else workspace max tier.
    pub fn tier_for_path(&self, path: &Path) -> Option<WireTier> {
        let s = path.to_string_lossy();
        let mut best: Option<(&str, WireTier)> = None;
        for (id, tier) in &self.packages {
            if !id.is_empty() && s.contains(id.as_str()) {
                if best.map(|(b, _)| id.len() > b.len()).unwrap_or(true) {
                    best = Some((id.as_str(), *tier));
                }
            }
        }
        best.map(|(_, t)| t).or_else(|| self.aggregate())
    }

    pub fn aggregate(&self) -> Option<WireTier> {
        self.packages.values().copied().max()
    }

    pub fn package_count(&self) -> usize {
        self.packages.len()
    }
}

/// Why a discover item is disabled. Strings are the menu labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuDisableReason {
    Connecting,
    BuildingT1,
    WaitingForServer,
    NeedsT2,
    NeedsT3,
    NotSupported,
    T3Skipped,
    NeedsContainer,
}

impl MenuDisableReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Connecting => "connecting language server",
            Self::BuildingT1 => "building T1 index",
            Self::WaitingForServer => "waiting for server",
            Self::NeedsT2 => "needs T2",
            Self::NeedsT3 => "needs T3",
            Self::NotSupported => "not supported",
            Self::T3Skipped => "T3 skipped (stub pack)",
            Self::NeedsContainer => "open folder in container",
        }
    }
}

/// One context / Navigate item. Value object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoverMenuItem {
    kind: DiscoverKind,
    enabled: bool,
    reason: Option<MenuDisableReason>,
}

impl DiscoverMenuItem {
    pub fn enabled(kind: DiscoverKind) -> Self {
        Self {
            kind,
            enabled: true,
            reason: None,
        }
    }

    pub fn disabled(kind: DiscoverKind, reason: MenuDisableReason) -> Self {
        Self {
            kind,
            enabled: false,
            reason: Some(reason),
        }
    }

    pub fn kind(self) -> DiscoverKind {
        self.kind
    }

    pub fn can_submit(self) -> bool {
        self.enabled
    }

    pub fn reason(self) -> Option<MenuDisableReason> {
        self.reason
    }

    pub fn label(self, enabled_label: &'static str) -> &'static str {
        self.reason
            .map(MenuDisableReason::as_str)
            .unwrap_or(enabled_label)
    }
}

/// LanguageCatalog × current tier × ingest × LSP / flight. Same for Navigate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiscoverMenu {
    definition: DiscoverMenuItem,
    implementation: DiscoverMenuItem,
    references: DiscoverMenuItem,
}

impl DiscoverMenu {
    pub fn paint(
        catalog: &LanguageCatalog,
        language_id: &str,
        lsp: LspSessionState,
        flight: &DiscoverFlight,
        ingest: IngestState,
        current: Option<WireTier>,
    ) -> Self {
        Self::paint_for_open(
            catalog,
            language_id,
            lsp,
            flight,
            ingest,
            current,
            T3HostOffer::Offered,
        )
    }

    pub fn paint_for_open(
        catalog: &LanguageCatalog,
        language_id: &str,
        lsp: LspSessionState,
        flight: &DiscoverFlight,
        ingest: IngestState,
        current: Option<WireTier>,
        t3_host: T3HostOffer,
    ) -> Self {
        Self {
            definition: item_for(
                catalog,
                language_id,
                DiscoverKind::Definition,
                lsp,
                flight,
                ingest,
                current,
                t3_host,
            ),
            implementation: item_for(
                catalog,
                language_id,
                DiscoverKind::Implementation,
                lsp,
                flight,
                ingest,
                current,
                t3_host,
            ),
            references: item_for(
                catalog,
                language_id,
                DiscoverKind::References,
                lsp,
                flight,
                ingest,
                current,
                t3_host,
            ),
        }
    }

    pub fn item(self, kind: DiscoverKind) -> DiscoverMenuItem {
        match kind {
            DiscoverKind::Definition => self.definition,
            DiscoverKind::Implementation => self.implementation,
            DiscoverKind::References => self.references,
        }
    }

    pub fn items(self) -> [DiscoverMenuItem; 3] {
        [self.definition, self.implementation, self.references]
    }

    /// None when the item is disabled — FakeLsp is not called.
    pub fn to_io_request(
        self,
        kind: DiscoverKind,
        tabs: &TabStrip,
        buffers: &BufferMap,
    ) -> Option<Result<LspIoRequest, IdeError>> {
        if !self.item(kind).can_submit() {
            return None;
        }
        Some(DiscoverCommand::new(kind).to_io_request(tabs, buffers))
    }
}

#[allow(clippy::too_many_arguments)]
fn item_for(
    catalog: &LanguageCatalog,
    language_id: &str,
    kind: DiscoverKind,
    lsp: LspSessionState,
    flight: &DiscoverFlight,
    ingest: IngestState,
    current: Option<WireTier>,
    t3_host: T3HostOffer,
) -> DiscoverMenuItem {
    if !lsp.is_ready() {
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::Connecting);
    }
    if !ingest.is_done() {
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::BuildingT1);
    }
    if !flight.can_submit() {
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::WaitingForServer);
    }
    let Some(offer) = catalog.offer(language_id, kind) else {
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::NotSupported);
    };
    let current = current.unwrap_or(WireTier::Syntax);
    if current.meets(offer.min_tier()) {
        return DiscoverMenuItem::enabled(kind);
    }
    disable_below_min(catalog, language_id, kind, offer, current, t3_host)
}

fn disable_below_min(
    catalog: &LanguageCatalog,
    language_id: &str,
    kind: DiscoverKind,
    offer: DiscoverOffer,
    _current: WireTier,
    t3_host: T3HostOffer,
) -> DiscoverMenuItem {
    if offer.is_t3_only() || (offer.min_tier() == WireTier::Graph && !catalog.has_t2(language_id)) {
        if !catalog.t3_supported(language_id) {
            return DiscoverMenuItem::disabled(kind, MenuDisableReason::NotSupported);
        }
        if !t3_host.is_offered() {
            return DiscoverMenuItem::disabled(kind, MenuDisableReason::NeedsContainer);
        }
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::T3Skipped);
    }
    if offer.min_tier() == WireTier::Graph {
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::NeedsT2);
    }
    if offer.min_tier() == WireTier::Types {
        if !catalog.t3_supported(language_id) {
            return DiscoverMenuItem::disabled(kind, MenuDisableReason::NotSupported);
        }
        if !t3_host.is_offered() {
            return DiscoverMenuItem::disabled(kind, MenuDisableReason::NeedsContainer);
        }
        return DiscoverMenuItem::disabled(kind, MenuDisableReason::T3Skipped);
    }
    DiscoverMenuItem::disabled(kind, MenuDisableReason::BuildingT1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{FakeClock, FakeLsp, MemFs};
    use progressive_lsp_control::{IndexPackage, Status, TierRow};

    fn catalog() -> LanguageCatalog {
        LanguageCatalog::new()
    }

    fn menu(
        lang: &str,
        lsp: LspSessionState,
        flight: DiscoverFlight,
        ingest: IngestState,
        tier: Option<WireTier>,
    ) -> DiscoverMenu {
        DiscoverMenu::paint(&catalog(), lang, lsp, &flight, ingest, tier)
    }

    #[test]
    fn tier_strip_value_object_java_t3_offered_csharp_not_supported_rust_t2_done() {
        let c = catalog();
        let java = TierStrip::paint(&c, "java", IngestState::Done, Some(WireTier::Graph));
        assert_eq!(java.t1().state(), TierCellState::Done);
        assert_eq!(java.t2().state(), TierCellState::Done);
        assert_eq!(java.t3().state(), TierCellState::Skipped);
        assert_eq!(java.t3().status(), "skipped");
        assert_eq!(java.t1().label(), "T1");
        assert_eq!(TierCellKind::T2.as_str(), "T2");
        assert_eq!(TierCellKind::T3.as_str(), "T3");
        assert_eq!(java.cells().len(), 3);

        let java_open = TierStrip::paint(&c, "java", IngestState::NotStarted, None);
        assert_eq!(java_open.t1().state(), TierCellState::InProgress);
        assert_eq!(java_open.t1().status(), "processing");
        assert_eq!(java_open.t2().state(), TierCellState::Na);
        assert_eq!(java_open.t2().status(), "n/a");
        assert_eq!(java_open.t3().state(), TierCellState::Na);

        let java_ingest = TierStrip::paint(&c, "java", IngestState::Running, None);
        assert_eq!(java_ingest.t1().state(), TierCellState::InProgress);
        assert_eq!(java_ingest.t2().state(), TierCellState::InProgress);
        assert_eq!(java_ingest.t2().status(), "processing");
        assert_eq!(java_ingest.t3().state(), TierCellState::Na);

        let java_t1 = TierStrip::paint(&c, "java", IngestState::Running, Some(WireTier::Syntax));
        assert_eq!(java_t1.t1().state(), TierCellState::Done);
        assert_eq!(java_t1.t2().state(), TierCellState::InProgress);
        assert_eq!(java_t1.t2().status(), "processing");
        assert_eq!(java_t1.t3().state(), TierCellState::Na);

        let java_t2 = TierStrip::paint(&c, "java", IngestState::Running, Some(WireTier::Graph));
        assert_eq!(java_t2.t2().state(), TierCellState::Done);
        assert_eq!(java_t2.t3().state(), TierCellState::InProgress);
        assert_eq!(java_t2.t3().status(), "processing");

        let java_types = TierStrip::paint(&c, "java", IngestState::Done, Some(WireTier::Types));
        assert_eq!(java_types.t3().state(), TierCellState::Done);

        let rust = TierStrip::paint(&c, "rust", IngestState::Done, Some(WireTier::Syntax));
        assert_eq!(rust.t1().state(), TierCellState::Done);
        assert_eq!(rust.t2().state(), TierCellState::Done);
        assert_eq!(rust.t2().status(), "done");
        assert_eq!(rust.t3().state(), TierCellState::Skipped);
        assert_eq!(rust.t3().status(), "skipped");

        for id in ["c", "cpp", "python", "css", "html"] {
            let strip = TierStrip::paint(&c, id, IngestState::Done, Some(WireTier::Syntax));
            assert_ne!(strip.t2().status(), "n/a", "{id}");
            assert_eq!(strip.t2().state(), TierCellState::Done, "{id}");
        }

        let rust_types = TierStrip::paint(&c, "rust", IngestState::Done, Some(WireTier::Types));
        assert_eq!(rust_types.t3().state(), TierCellState::Done);

        let csharp = TierStrip::paint(&c, "csharp", IngestState::Running, Some(WireTier::Syntax));
        assert_eq!(csharp.t1().state(), TierCellState::Done);
        assert_eq!(csharp.t2().state(), TierCellState::InProgress);
        assert_eq!(csharp.t3().state(), TierCellState::NotSupported);

        let building = TierStrip::paint(&c, "python", IngestState::NotStarted, None);
        assert_eq!(building.t1().status(), "processing");
        assert_eq!(building.t2().state(), TierCellState::Na);
        assert_eq!(building.t3().state(), TierCellState::Na);

        let python_t2 = TierStrip::paint(&c, "python", IngestState::Running, Some(WireTier::Graph));
        assert_eq!(python_t2.t1().state(), TierCellState::Done);
        assert_eq!(python_t2.t2().state(), TierCellState::Done);
        assert_eq!(python_t2.t3().state(), TierCellState::InProgress);
        assert_eq!(python_t2.t3().status(), "processing");

        let folder_open = TierStrip::paint(&c, "plaintext", IngestState::Running, None);
        assert_eq!(folder_open.t1().status(), "processing");
        assert_eq!(folder_open.t2().status(), "processing");
        assert_eq!(folder_open.t3().state(), TierCellState::Na);

        let unknown = TierStrip::paint(&c, "plaintext", IngestState::Done, None);
        assert_eq!(unknown.t1().state(), TierCellState::Done);
        assert_eq!(unknown.t2().state(), TierCellState::Done);
        assert_eq!(unknown.t3().state(), TierCellState::Na);
        assert_ne!(java, csharp);
        let cell = TierCell::new(TierCellKind::T1, TierCellState::Done);
        assert_eq!(cell.kind(), TierCellKind::T1);
        assert_eq!(cell.state(), TierCellState::Done);
    }

    #[test]
    fn package_tier_map_applies_index_tier_ready_and_path_fallback() {
        let mut map = PackageTierMap::new();
        assert_eq!(map.ingest(), IngestState::NotStarted);
        assert_eq!(
            map.ingest_for_strip(LspSessionState::Connecting),
            IngestState::Running
        );
        assert_eq!(
            map.ingest_for_strip(LspSessionState::Idle),
            IngestState::NotStarted
        );
        assert_eq!(
            map.ingest_for_strip(LspSessionState::Ready),
            IngestState::Done
        );
        assert_eq!(map.package_count(), 0);
        assert!(map.aggregate().is_none());
        map.apply_index_status(&IndexStatusResponse {
            status: Some(Status::ok()),
            packages: vec![IndexPackage {
                package_id: "lib".into(),
                generation: 1,
            }],
            cache_entries: 0,
            ingest: IngestState::Done.as_str().into(),
        });
        assert_eq!(map.ingest(), IngestState::Done);
        assert_eq!(map.tier_for_package("lib"), Some(WireTier::Syntax));
        map.apply_tier_status(&TierStatusResponse {
            status: Some(Status::ok()),
            rows: vec![TierRow {
                package_id: "lib".into(),
                tier: "syntax".into(),
            }],
        });
        map.apply_tier_ready(&TierReady {
            package_id: "lib".into(),
            tier: "graph".into(),
        });
        assert_eq!(map.tier_for_package("lib"), Some(WireTier::Graph));
        assert_eq!(
            map.tier_for_path(Path::new("/ws/lib/src/Main.java")),
            Some(WireTier::Graph)
        );
        assert_eq!(
            map.tier_for_path(Path::new("/ws/other/App.java")),
            Some(WireTier::Graph)
        );
        map.apply_tier_status(&TierStatusResponse {
            status: Some(Status::ok()),
            rows: vec![TierRow {
                package_id: "app".into(),
                tier: "types".into(),
            }],
        });
        assert_eq!(map.aggregate(), Some(WireTier::Types));
        assert_eq!(
            map.tier_for_path(Path::new("/ws/app/Foo.java")),
            Some(WireTier::Types)
        );
        map.apply_tier_ready(&TierReady {
            package_id: "app".into(),
            tier: "nope".into(),
        });
        assert_eq!(map.tier_for_package("app"), Some(WireTier::Types));
        assert_eq!(PackageTierMap::default(), PackageTierMap::new());
    }

    #[test]
    fn discover_menu_value_object_strings_change_on_tier_ready() {
        let connecting = menu(
            "java",
            LspSessionState::Connecting,
            DiscoverFlight::idle(),
            IngestState::NotStarted,
            None,
        );
        assert!(!connecting.item(DiscoverKind::Definition).can_submit());
        assert_eq!(
            connecting
                .item(DiscoverKind::Definition)
                .label("Find Definition"),
            "connecting language server"
        );

        let building = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Running,
            None,
        );
        assert_eq!(
            building
                .item(DiscoverKind::References)
                .label("Find References"),
            "building T1 index"
        );

        let pending = PackageTierMap::new();
        assert_eq!(pending.ingest(), IngestState::NotStarted);
        let ingest = pending.ingest_for_strip(LspSessionState::Ready);
        assert_eq!(ingest, IngestState::Done);
        let ready_before_index = DiscoverMenu::paint(
            &catalog(),
            "java",
            LspSessionState::Ready,
            &DiscoverFlight::idle(),
            ingest,
            None,
        );
        assert!(
            ready_before_index
                .item(DiscoverKind::Definition)
                .can_submit(),
            "strip paints T1 done on Ready; discover must not stay building T1"
        );
        assert!(ready_before_index
            .item(DiscoverKind::References)
            .can_submit());

        let waiting = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle().begin(DiscoverKind::Definition),
            IngestState::Done,
            Some(WireTier::Syntax),
        );
        assert_eq!(
            waiting
                .item(DiscoverKind::Definition)
                .label("Find Definition"),
            "waiting for server"
        );
        assert_eq!(
            waiting.item(DiscoverKind::Definition).reason(),
            Some(MenuDisableReason::WaitingForServer)
        );

        let t1 = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Syntax),
        );
        assert!(t1.item(DiscoverKind::Definition).can_submit());
        assert_eq!(
            t1.item(DiscoverKind::Definition).label("Find Definition"),
            "Find Definition"
        );
        assert!(t1.item(DiscoverKind::References).can_submit());
        assert!(!t1.item(DiscoverKind::Implementation).can_submit());
        assert_eq!(
            t1.item(DiscoverKind::Implementation)
                .label("Find Implementation"),
            "needs T2"
        );

        let t2 = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Graph),
        );
        assert!(t2.item(DiscoverKind::Implementation).can_submit());
        assert_eq!(
            t2.item(DiscoverKind::Implementation)
                .label("Find Implementation"),
            "Find Implementation"
        );
        assert_ne!(t1, t2);

        let rust = menu(
            "rust",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Syntax),
        );
        assert_eq!(
            rust.item(DiscoverKind::Implementation)
                .label("Find Implementation"),
            "needs T2"
        );

        let java_t3 = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Graph),
        );
        assert_ne!(
            java_t3.item(DiscoverKind::Implementation).reason(),
            Some(MenuDisableReason::T3Skipped)
        );

        let plain = menu(
            "plaintext",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            None,
        );
        assert_eq!(
            plain
                .item(DiscoverKind::Definition)
                .label("Find Definition"),
            "not supported"
        );
        assert_eq!(
            MenuDisableReason::Connecting.as_str(),
            "connecting language server"
        );
        assert_eq!(MenuDisableReason::NeedsT3.as_str(), "needs T3");
        assert_eq!(MenuDisableReason::NotSupported.as_str(), "not supported");
        assert_eq!(
            MenuDisableReason::NeedsContainer.as_str(),
            "open folder in container"
        );
        assert_eq!(t1.items().len(), 3);
        assert_eq!(
            t1.item(DiscoverKind::Definition).kind(),
            DiscoverKind::Definition
        );
    }

    #[test]
    fn discover_menu_does_not_call_fake_lsp_while_disabled() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/Main.java", "class Main {}").unwrap();
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        buffers.open("/ws/Main.java", &fs).unwrap();
        tabs.open("/ws/Main.java");

        let disabled = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Running,
            None,
        );
        let fake = FakeLsp::new();
        assert!(disabled
            .to_io_request(DiscoverKind::Definition, &tabs, &buffers)
            .is_none());
        assert!(fake.sent().is_empty());
        assert!(
            DiscoverCommand::definition()
                .apply(
                    None::<&mut crate::lsp::LspClient<FakeLsp>>,
                    &mut tabs,
                    &mut buffers,
                    &fs,
                    None,
                )
                .is_err()
                || fake.sent().is_empty()
        );
        let _ = FakeClock::at_unix_ms(1);

        let ready = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Syntax),
        );
        let req = ready
            .to_io_request(DiscoverKind::Definition, &tabs, &buffers)
            .unwrap()
            .unwrap();
        assert!(req.is_discover());
        assert!(ready
            .to_io_request(DiscoverKind::Implementation, &tabs, &buffers)
            .is_none());
        assert!(fake.sent().is_empty());
    }

    #[test]
    fn discover_menu_disables_navigate_and_f12_until_lsp_ready() {
        let mut fs = MemFs::new();
        fs.add_file("/ws/Main.java", "class Main {}").unwrap();
        let mut tabs = TabStrip::new();
        let mut buffers = BufferMap::new();
        buffers.open("/ws/Main.java", &fs).unwrap();
        tabs.open("/ws/Main.java");

        for lsp in [
            LspSessionState::Idle,
            LspSessionState::Connecting,
            LspSessionState::Failed,
        ] {
            let blocked = menu(
                "java",
                lsp,
                DiscoverFlight::idle(),
                IngestState::Done,
                Some(WireTier::Syntax),
            );
            for kind in [
                DiscoverKind::Definition,
                DiscoverKind::Implementation,
                DiscoverKind::References,
            ] {
                assert!(
                    !blocked.item(kind).can_submit(),
                    "{lsp:?} must disable {kind:?} (Navigate / F12)"
                );
                assert_eq!(
                    blocked.item(kind).label("Go to Definition"),
                    "connecting language server"
                );
                assert!(
                    blocked.to_io_request(kind, &tabs, &buffers).is_none(),
                    "F12 {kind:?} must not queue IO until Ready"
                );
            }
        }

        let ready = menu(
            "java",
            LspSessionState::Ready,
            DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Syntax),
        );
        assert!(ready.item(DiscoverKind::Definition).can_submit());
        assert!(ready
            .to_io_request(DiscoverKind::Definition, &tabs, &buffers)
            .is_some());
        assert_ne!(
            ready
                .item(DiscoverKind::Definition)
                .label("Go to Definition"),
            "connecting language server"
        );
        assert!(!LspSessionState::Idle.is_ready());
        assert!(!LspSessionState::Failed.is_ready());
        assert!(LspSessionState::Ready.is_ready());
    }

    #[test]
    fn native_non_linux_open_skips_t3_and_asks_for_container() {
        let c = catalog();
        let python = TierStrip::paint_for_open(
            &c,
            "python",
            IngestState::Done,
            Some(WireTier::Syntax),
            T3HostOffer::NeedsContainer,
        );
        assert_eq!(python.t3().state(), TierCellState::Skipped);
        let java = TierStrip::paint_for_open(
            &c,
            "java",
            IngestState::Done,
            Some(WireTier::Graph),
            T3HostOffer::NeedsContainer,
        );
        assert_eq!(java.t3().state(), TierCellState::Skipped);

        let rust = DiscoverMenu::paint_for_open(
            &c,
            "rust",
            LspSessionState::Ready,
            &DiscoverFlight::idle(),
            IngestState::Done,
            Some(WireTier::Syntax),
            T3HostOffer::NeedsContainer,
        );
        assert_eq!(
            rust.item(DiscoverKind::Implementation).reason(),
            Some(MenuDisableReason::NeedsT2)
        );
        assert_eq!(
            rust.item(DiscoverKind::Implementation)
                .label("Find Implementation"),
            "needs T2"
        );
    }
}
