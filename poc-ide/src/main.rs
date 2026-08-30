//! Composition root: eframe + `rfd`. The lib takes Ports only.

mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let launch = parse_args(std::env::args().skip(1));
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
            Ok(Box::new(ui::PocIdeApp::new(
                launch.folder,
                launch.file,
                launch.control_socket,
            )))
        }),
    )
}

struct Launch {
    folder: Option<PathBuf>,
    file: Option<PathBuf>,
    control_socket: Option<PathBuf>,
}

fn parse_args(args: impl Iterator<Item = String>) -> Launch {
    let mut folder = None;
    let mut file = None;
    let mut control_socket = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--folder=") {
            folder = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--file=") {
            file = Some(PathBuf::from(value));
        } else if let Some(value) = arg.strip_prefix("--control-socket=") {
            control_socket = Some(PathBuf::from(value));
        } else if arg == "--folder" {
            folder = args.next().map(PathBuf::from);
        } else if arg == "--file" {
            file = args.next().map(PathBuf::from);
        } else if arg == "--control-socket" {
            match args.peek() {
                Some(next) if !next.starts_with('-') => {
                    control_socket = args.next().map(PathBuf::from);
                }
                _ => {
                    control_socket = Some(std::env::temp_dir().join("poc-ide-control.sock"));
                }
            }
        }
    }
    Launch {
        folder,
        file,
        control_socket,
    }
}
