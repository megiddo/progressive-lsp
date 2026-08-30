//! Composition root: eframe + `rfd`. The lib takes Ports only.

mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let (folder, file) = parse_args(std::env::args().skip(1));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("poc-ide")
            .with_inner_size([1100.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "poc-ide",
        options,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(ui::PocIdeApp::new(folder, file)))
        }),
    )
}

fn parse_args(args: impl Iterator<Item = String>) -> (Option<PathBuf>, Option<PathBuf>) {
    let mut folder = None;
    let mut file = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--folder=") {
            folder = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--file=") {
            file = Some(PathBuf::from(value));
        } else if arg == "--folder" {
            folder = args.next().map(PathBuf::from);
        } else if arg == "--file" {
            file = args.next().map(PathBuf::from);
        }
    }
    (folder, file)
}
