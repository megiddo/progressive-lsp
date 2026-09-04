//! Composition root: eframe + `rfd`. The lib takes Ports only.

mod ui;

use poc_ide::{parse_launch_args, ControlSocketPath, RunLog, SystemClock};

fn main() -> eframe::Result<()> {
    let launch = parse_launch_args(std::env::args().skip(1));
    let run_log = match RunLog::open_default(SystemClock) {
        Ok(log) => log,
        Err(_) => RunLog::memory(SystemClock).unwrap_or_else(|_| RunLog::unavailable(SystemClock)),
    };
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
            let open_mode = launch.open_mode();
            Ok(Box::new(ui::PocIdeApp::new(
                launch.folder,
                launch.file,
                Some(
                    ControlSocketPath::resolve_default(launch.control_socket.as_deref())
                        .into_path(),
                ),
                run_log,
                open_mode,
            )))
        }),
    )
}
