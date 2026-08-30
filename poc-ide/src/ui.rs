//! eframe view. Ignored by llvm-cov. Domain state lives in the lib.

use std::path::PathBuf;

use eframe::egui;
use poc_ide::{
    DialogPort, FileTree, FsPort, LayoutState, StdFs, TabStrip, TreeNode, WorkspaceRoot,
};

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

pub struct PocIdeApp {
    dialog: RfdDialog,
    fs: StdFs,
    root: Option<WorkspaceRoot>,
    tree: Option<FileTree>,
    tabs: TabStrip,
    layout: LayoutState,
    status: String,
}

impl PocIdeApp {
    pub fn new(folder: Option<PathBuf>, file: Option<PathBuf>) -> Self {
        let mut app = Self {
            dialog: RfdDialog,
            fs: StdFs,
            root: None,
            tree: None,
            tabs: TabStrip::new(),
            layout: LayoutState::new(),
            status: String::new(),
        };
        if let Some(dir) = folder {
            app.apply_folder_path(&dir);
        }
        if let Some(path) = file {
            app.apply_file_path(&path);
        }
        app
    }

    fn apply_folder_path(&mut self, path: &std::path::Path) {
        match WorkspaceRoot::from_folder_path(path, &self.fs) {
            Ok(root) => self.set_root(root, None),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn apply_file_path(&mut self, path: &std::path::Path) {
        match WorkspaceRoot::from_file_path(path, &self.fs) {
            Ok((root, file)) => self.set_root(root, Some(file)),
            Err(e) => self.status = e.to_string(),
        }
    }

    fn set_root(&mut self, root: WorkspaceRoot, open_file: Option<PathBuf>) {
        match self.fs.read_tree(&root) {
            Ok(tree) => {
                if self.root.as_ref() != Some(&root) {
                    self.tabs = TabStrip::new();
                }
                self.root = Some(root);
                self.tree = Some(tree);
                self.status.clear();
                if let Some(file) = open_file {
                    self.tabs.open(file);
                }
            }
            Err(e) => self.status = e.to_string(),
        }
    }

    fn pick_folder(&mut self) {
        if let Ok(Some(root)) = WorkspaceRoot::open_folder(&mut self.dialog, &self.fs) {
            self.set_root(root, None);
        }
    }

    fn pick_file(&mut self) {
        if let Ok(Some((root, file))) = WorkspaceRoot::open_file(&mut self.dialog, &self.fs) {
            self.set_root(root, Some(file));
        }
    }
}

impl eframe::App for PocIdeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            show_nodes(ui, &nodes, &mut self.tabs);
                        });
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
                        if ui.selectable_label(selected, tab.label()).clicked() {
                            self.tabs.focus(tab);
                        }
                        if ui.small_button("×").clicked() {
                            self.tabs.close(tab);
                        }
                    }
                }
            });
            ui.separator();
            match self.tabs.focused() {
                Some(id) => {
                    ui.label(id.as_path().display().to_string());
                    ui.weak("Empty editor pane — buffers land in IDE-2.");
                }
                None => {
                    ui.label("No file open");
                }
            }
        });
    }
}

fn show_nodes(ui: &mut egui::Ui, nodes: &[TreeNode], tabs: &mut TabStrip) {
    for node in nodes {
        if node.is_dir() {
            egui::CollapsingHeader::new(node.name())
                .default_open(true)
                .show(ui, |ui| {
                    show_nodes(ui, node.children(), tabs);
                });
        } else if ui.selectable_label(false, node.name()).clicked() {
            tabs.open(node.path());
        }
    }
}
