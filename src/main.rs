#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod app;
mod connection;
mod design;
mod support;
mod toolbox_ui;

fn main() -> eframe::Result {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|v| v == "--device-task")
    {
        let path = std::env::args_os().nth(2).expect("task file");
        std::process::exit(apple_toolkit::actions::worker_main(std::path::Path::new(
            &path,
        )));
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|v| v == "--mdm-worker")
    {
        let args: Vec<_> = std::env::args().collect();
        let actions: Vec<_> = args
            .windows(2)
            .filter(|p| p[0] == "--action")
            .map(|p| p[1].as_str())
            .collect();
        if actions.len() != 1 || !["probe", "inspection"].contains(&actions[0]) {
            eprintln!("[STOP] 此版本只支持设备识别和详细信息读取。");
            std::process::exit(1);
        }
        std::process::exit(apple_toolkit::mdm::worker_main());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_icon(design::window_icon())
            .with_inner_size([1280.0, 780.0])
            .with_min_inner_size([1060.0, 680.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Unbound · 设备管理",
        options,
        Box::new(|cc| Ok(Box::new(app::ToolkitApp::new(cc)))),
    )
}
