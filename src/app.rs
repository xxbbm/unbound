#[path = "presentation.rs"]
mod presentation;
use apple_toolkit::{
    bundled,
    mdm::{self, Device},
    runner::{self, Event, Run},
};
use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
enum Job {
    Probe,
    Inspection,
    Other,
}

/// Four user-facing pages share one device identity and one foreground job.
pub struct ToolkitApp {
    runtime: Option<PathBuf>,
    initializing: Option<Receiver<Result<PathBuf, String>>>,
    init_error: Option<String>,
    device: Option<Device>,
    run: Option<Run>,
    job: Option<Job>,
    last_probe: Instant,
    connection: crate::connection::Monitor,
    closing: bool,
    status: String,
    protocol: String,
    last_error: String,
    outcome: Option<Result<String, String>>,
    logs: VecDeque<(bool, String)>,
    log_bytes: usize,
    tab: usize,
    report: Option<apple_toolkit::inspection::Report>,
    photo: Option<(String, egui::TextureHandle)>,
    toolbox: crate::toolbox_ui::Center,
    search: String,
    device_search: String,
    show_common: bool,
    show_help: bool,
    show_about: bool,
    /// 标题下方的平台选择：自动 / iOS / Android。
    platform: apple_toolkit::platform::Selection,
    /// 上一次扫描到的安卓设备（自动模式据此判断）。
    android_devices: Vec<apple_toolkit::platform::AdbDevice>,
    /// 安卓侧独立轮询；与 iOS 的连接监视互不影响。
    android_scan: crate::connection::AndroidMonitor,
    /// 选中安卓设备后的只读摘要（机型/电量/存储/屏幕）。
    android_status: Option<apple_toolkit::android::Status>,
    android_probe: crate::connection::AndroidProbe,
    /// 累加的界面使用时长（秒），用于决定何时弹支持提示。
    usage_seconds: f32,
    support_open: bool,
    /// 本会话已经弹过一次，关掉后不再自动弹。
    support_shown: bool,
    /// 用户点过「不再提示」后置真，并写进磁盘，下次启动不再弹。
    support_dismissed: bool,
    support_tex: Option<(egui::TextureHandle, egui::TextureHandle)>,
    /// 放大查看的是哪一张：0 = 微信赞赏，1 = 支付宝。
    support_zoom: Option<u8>,
    photo_failed: Option<String>,
    notice: String,
    content_height: f32,
    brand: egui::TextureHandle,
}
impl ToolkitApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // A slightly denser default while respecting the operating system DPI.
        cc.egui_ctx.set_zoom_factor(0.9);
        install_font(&cc.egui_ctx);
        crate::design::install(&cc.egui_ctx);
        // 把内置的 adb 解到用户数据目录；失败只影响安卓功能，不阻塞启动
        let _ = apple_toolkit::android::ensure_tools();
        let mut app = Self {
            runtime: None,
            initializing: None,
            init_error: None,
            device: None,
            run: None,
            job: None,
            last_probe: Instant::now() - Duration::from_secs(30),
            connection: Default::default(),
            closing: false,
            status: "正在准备应用…".into(),
            protocol: String::new(),
            last_error: String::new(),
            outcome: None,
            logs: VecDeque::new(),
            log_bytes: 0,
            tab: 1,
            report: None,
            photo: None,
            toolbox: Default::default(),
            search: String::new(),
            device_search: String::new(),
            show_common: false,
            show_help: false,
            platform: Default::default(),
            android_devices: Vec::new(),
            android_scan: Default::default(),
            android_status: None,
            android_probe: Default::default(),
            usage_seconds: 0.0,
            support_open: false,
            support_shown: false,
            support_dismissed: crate::support::dismissed(),
            support_tex: None,
            support_zoom: None,
            show_about: false,
            photo_failed: None,
            notice: String::new(),
            content_height: 620.0,
            brand: crate::design::load_logo(&cc.egui_ctx),
        };
        app.initialize();
        app
    }
    fn initialize(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.initializing = Some(rx);
        self.init_error = None;
        std::thread::spawn(move || {
            let _ = tx.send(bundled::ensure_runtime());
        });
    }
    fn log(&mut self, err: bool, text: String) {
        self.log_bytes += text.len();
        self.logs.push_back((err, text));
        while self.log_bytes > 1_048_576 {
            if let Some((_, s)) = self.logs.pop_front() {
                self.log_bytes -= s.len();
            } else {
                break;
            }
        }
    }
    fn poll(&mut self) {
        if let Some(rx) = &self.initializing {
            match rx.try_recv() {
                Ok(Ok(root)) => {
                    self.runtime = Some(root);
                    self.initializing = None;
                    self.status = "正在查找设备…".into();
                }
                Ok(Err(e)) => {
                    self.init_error = Some(e);
                    self.initializing = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.init_error = Some("应用初始化中断，请重新打开应用。".into());
                    self.initializing = None;
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        for _ in 0..512 {
            let event = match self.run.as_ref().map(|r| r.events.try_recv()) {
                Some(Ok(e)) => e,
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.finish(false, "设备任务通信中断，请重试。".into());
                    break;
                }
                _ => break,
            };
            match event {
                Event::Started(_) => (),
                Event::Output { stderr, text } => {
                    self.protocol.push_str(&text);
                    while let Some(n) = self.protocol.find('\n') {
                        let line = self.protocol[..n].trim().to_owned();
                        self.protocol.drain(..=n);
                        self.protocol_line(&line);
                    }
                    if self.protocol.len() > 512 * 1024 {
                        self.protocol.clear();
                    }
                    self.log(stderr, text);
                }
                Event::Error(e) => {
                    self.finish(false, e);
                    break;
                }
                Event::Finished {
                    code,
                    cancelled,
                    timed_out,
                    dropped,
                } => {
                    let error = if cancelled {
                        "操作已停止。已写入设备的内容不会自动撤销。".into()
                    } else if timed_out {
                        "等待设备响应超时，请检查连接后重试。".into()
                    } else if !self.last_error.is_empty() {
                        self.last_error.clone()
                    } else {
                        format!("设备操作未完成（退出码 {code:?}），可展开诊断信息查看原因。")
                    };
                    if dropped > 0 {
                        self.log(true, format!("[日志有{dropped}个块未保留]\n"));
                    }
                    let stopped_monitor = cancelled
                        && self.job == Some(Job::Other)
                        && self.toolbox.selected == "device-monitor";
                    self.finish(
                        (code == Some(0) && !cancelled && !timed_out) || stopped_monitor,
                        error,
                    );
                    break;
                }
            }
        }
    }
    fn start_action(&mut self, task: apple_toolkit::actions::Task) {
        use std::io::Write;
        if self.run.is_some() {
            return;
        }
        self.connection.pause();
        let result = (|| -> Result<(PathBuf, PathBuf), String> {
            task.validate()?;
            let directory = mdm::default_workspace().join("jobs");
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos();
            let path = directory.join(format!("task-{}-{stamp}.json", std::process::id()));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            self.toolbox.task_path = Some(path.clone());
            let mut file = options.open(&path).map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec(&task).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            Ok((std::env::current_exe().map_err(|e| e.to_string())?, path))
        })();
        match result {
            Ok((program, path)) => {
                self.logs.clear();
                self.log_bytes = 0;
                self.protocol.clear();
                self.last_error.clear();
                self.outcome = None;
                self.job = Some(Job::Other);
                self.status = format!(
                    "正在运行：{}",
                    apple_toolkit::actions::catalog()
                        .iter()
                        .find(|a| a.id == task.id)
                        .map(|a| a.name)
                        .unwrap_or("设备操作")
                );
                self.run = Some(runner::start(runner::Request {
                    program,
                    args: vec!["--device-task".into(), path.into_os_string()],
                    timeout: Duration::from_secs(if task.id == "device-monitor" {
                        86400
                    } else {
                        7500
                    }),
                }));
            }
            Err(e) => {
                self.toolbox.finished();
                self.outcome = Some(Err(e));
            }
        }
    }

    fn protocol_line(&mut self, line: &str) {
        if self.job == Some(Job::Other) {
            self.toolbox.protocol(line);
            if let Some(s) = line.strip_prefix("[STEP] ") {
                self.status = s.into();
            }
            if let Some(s) = line.strip_prefix("[STOP] ") {
                self.last_error = s.into();
            }
            return;
        }
        if let Some(json) = line.strip_prefix("[DEVICE] ") {
            if let Ok(d) = serde_json::from_str::<Device>(json) {
                if self
                    .device
                    .as_ref()
                    .is_none_or(|old| !old.same_identity(&d))
                {
                    self.report = None;
                    self.photo = None;
                    self.outcome = None;
                    self.toolbox.clear_device();
                }
                self.device = Some(d);
            }
        } else if let Some(json) = line.strip_prefix("[REPORT] ") {
            if let Ok(report) = serde_json::from_str(json) {
                self.report = Some(report);
            }
        } else if let Some(s) = line.strip_prefix("[STEP] ") {
            self.status = s.into();
        } else if let Some(s) = line.strip_prefix("[STOP] ") {
            self.last_error = s.into();
        }
    }
    fn finish(&mut self, success: bool, error: String) {
        let job = self.job.take();
        self.toolbox.finished();
        self.run = None;
        self.last_probe = Instant::now();
        match job {
            Some(Job::Probe) => {
                if success {
                    self.status = "设备已连接".into();
                } else {
                    self.device = None;
                    self.report = None;
                    self.photo = None;
                    self.toolbox.clear_device();
                    self.status = "等待手机连接".into();
                    self.last_error = error;
                }
            }
            Some(Job::Inspection) => {
                self.status = if success {
                    "详细信息读取完成"
                } else {
                    "部分信息未能读取"
                }
                .into();
                if !success {
                    if let Some(r) = &mut self.report {
                        r.notes.push(format!("补充读取未完成：{error}"));
                    } else {
                        self.last_error = error;
                    }
                }
            }
            Some(Job::Other) => {
                self.status = if success {
                    "操作完成"
                } else {
                    "操作已结束"
                }
                .into();
                self.outcome = Some(if success {
                    Ok(self.toolbox.message.clone())
                } else {
                    Err(error)
                });
            }
            None => (),
        }
    }
    fn start_mdm(&mut self, job: Job, snapshot: Option<Device>) {
        if self.run.is_some() {
            return;
        }
        let Some(root) = &self.runtime else { return };
        self.connection.pause();
        let action = match job {
            Job::Probe => "probe",
            Job::Inspection => "inspection",
            Job::Other => return,
        };
        let mut args = vec![
            "--mdm-worker".into(),
            "--action".into(),
            action.into(),
            "--source-root".into(),
            root.as_os_str().into(),
        ];
        if let Some(d) = snapshot.or_else(|| self.device.clone()) {
            args.extend(["--udid".into(), d.udid.into()]);
        }
        if job == Job::Inspection {
            self.logs.clear();
            self.log_bytes = 0;
        }
        self.last_error.clear();
        self.protocol.clear();
        self.job = Some(job);
        self.status = if job == Job::Probe {
            "正在识别手机…"
        } else {
            "正在读取设备详细信息…"
        }
        .into();
        match std::env::current_exe() {
            Ok(program) => {
                self.run = Some(runner::start(runner::Request {
                    program,
                    args,
                    timeout: Duration::from_secs(if job == Job::Probe { 30 } else { 90 }),
                }))
            }
            Err(e) => self.finish(false, e.to_string()),
        }
    }
    fn read_details(&mut self) {
        self.tab = 2;
        if self.run.is_none() {
            if let Some(d) = self.device.clone() {
                self.report = Some(apple_toolkit::inspection::Report::base(d.clone()));
                self.start_mdm(Job::Inspection, Some(d));
            }
        }
    }
    fn tool_page(&mut self, ui: &mut egui::Ui, group: &str) {
        self.toolbox.choose_group(group);
        if let Some(task) = self
            .toolbox
            .show(ui, self.device.as_ref(), self.run.is_some())
        {
            self.start_action(task);
        }
        if let Some(Err(error)) = &self.outcome {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
    }
    fn details(&mut self, ui: &mut egui::Ui) {
        ui.heading("详细验机");
        ui.label("查看当前手机实际返回的信息，未获取的项目会明确显示。");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.device.is_some() && self.run.is_none(),
                    egui::Button::new("读取 / 刷新详细信息").min_size(egui::vec2(220.0, 40.0)),
                )
                .clicked()
            {
                self.read_details();
            }
            if self.job == Some(Job::Inspection) {
                ui.spinner();
                ui.label("正在读取…");
            }
        });
        let Some(report) = self.report.clone() else {
            ui.add_space(16.0);
            ui.label("连接手机后，点击上方按钮开始读取。");
            return;
        };
        let read = report
            .rows
            .iter()
            .filter(|r| r.result.starts_with("已读取"))
            .count();
        ui.label(format!(
            "已获取 {read}/{} 项 · {}",
            report.rows.len(),
            report.read_at_label()
        ));
        ui.horizontal(|ui| {
            if ui.button("保存报告（HTML）").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("Unbound设备报告.html")
                    .add_filter("HTML报告", &["html"])
                    .save_file()
                {
                    if let Err(e) = std::fs::write(path, report.html()) {
                        self.last_error = e.to_string();
                    }
                }
            }
            if ui.button("导出数据（JSON）").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("Unbound设备数据.json")
                    .add_filter("JSON数据", &["json"])
                    .save_file()
                {
                    match serde_json::to_vec_pretty(&report) {
                        Ok(bytes) => {
                            if let Err(e) = std::fs::write(path, bytes) {
                                self.last_error = e.to_string();
                            }
                        }
                        Err(e) => self.last_error = e.to_string(),
                    }
                }
            }
        });
        ui.separator();
        let reading_width = (ui.available_width() - 260.0).max(180.0);
        egui::Grid::new("inspection-readings")
            .striped(true)
            .num_columns(3)
            .spacing([12.0, 10.0])
            .show(ui, |ui| {
                for (title, width) in [
                    ("检测项目", 116.0),
                    ("实际读取值", reading_width),
                    ("读取说明", 120.0),
                ] {
                    ui.add_sized(
                        [width, 28.0],
                        egui::Label::new(egui::RichText::new(title).strong()),
                    );
                }
                ui.end_row();
                for row in &report.rows {
                    ui.add_sized([116.0, 28.0], egui::Label::new(&row.item).wrap());
                    ui.add_sized(
                        [reading_width, 28.0],
                        egui::Label::new(&row.current).selectable(true).wrap(),
                    )
                    .on_hover_text(&row.source);
                    ui.add_sized([120.0, 28.0], egui::Label::new(&row.result).wrap());
                    ui.end_row();
                }
            });
        ui.collapsing("读取说明", |ui| {
            for note in &report.notes {
                ui.small(note);
            }
        });
        if !self.last_error.is_empty() {
            ui.colored_label(ui.visuals().error_fg_color, &self.last_error);
        }
    }
}
impl eframe::App for ToolkitApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.poll();
        ctx.request_repaint_after(Duration::from_millis(150));
        if ctx.input(|i| i.viewport().close_requested()) && self.run.is_some() {
            self.closing = true;
            if let Some(r) = &self.run {
                self.toolbox.stop(r);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if self.closing && self.run.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if !self.closing && self.run.is_none() {
            // 安卓侧：独立轮询，失败只保留旧值，不影响 iOS 流程。
            if let Some(devices) = self.android_scan.tick() {
                self.android_devices = devices;
            }
            // 安卓摘要：有结果就收下；缺了就补一次后台探测，不阻塞界面。
            if let Some(result) = self.android_probe.poll() {
                self.android_status = result.ok();
            }
            if self.active_platform() == Some(apple_toolkit::platform::Platform::Android)
                && self.android_status.is_none()
                && !self.android_probe.busy()
            {
                if let Some(serial) = self.android_serial() {
                    self.android_probe.start(serial);
                }
            }
            if let Some(root) = &self.runtime {
                use crate::connection::Change;
                match self
                    .connection
                    .tick(root, self.device.as_ref().map(|d| d.udid.as_str()))
                {
                    Change::Read if self.last_probe.elapsed() >= Duration::from_secs(15) => {
                        self.device = None;
                        self.start_mdm(Job::Probe, None);
                    }
                    Change::Disconnected | Change::Multiple => {
                        self.device = None;
                        self.report = None;
                        self.photo = None;
                        self.toolbox.clear_device();
                        self.status = "请只连接一台手机".into();
                    }
                    _ => (),
                }
            }
        }
        let screen = ctx.screen_rect();
        let wide = screen.width() >= 1180.0;
        crate::design::wash(
            &ctx.layer_painter(egui::LayerId::background()),
            screen,
            egui::Color32::from_rgb(249, 250, 253),
            egui::Color32::from_rgb(235, 239, 246),
        );
        egui::TopBottomPanel::top("header")
            .show_separator_line(false)
            .exact_height(94.0)
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_white_alpha(70))
                    .inner_margin(egui::Margin {
                        left: 26,
                        right: 26,
                        top: 16,
                        bottom: 12,
                    }),
            )
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::bottom("task-status")
            .show_separator_line(false)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 26,
                right: 26,
                top: 4,
                bottom: 7,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().interact_size.y = 22.0;
                ui.horizontal(|ui| {
                    if self.run.is_some() {
                        ui.spinner();
                        ui.label(egui::RichText::new(&self.status).size(13.0));
                        if ui.small_button("停止当前任务").clicked() {
                            if let Some(r) = &self.run {
                                self.toolbox.stop(r);
                            }
                        }
                    } else {
                        crate::design::status(
                            ui,
                            self.device.is_some(),
                            if self.device.is_some() {
                                "USB 连接正常"
                            } else {
                                "等待 USB 设备连接"
                            },
                        );
                        if !self.notice.is_empty() {
                            ui.label(
                                egui::RichText::new(&self.notice)
                                    .size(13.0)
                                    .color(crate::design::BLUE),
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(concat!("Unbound ", env!("CARGO_PKG_VERSION")))
                                .size(12.0)
                                .color(crate::design::MUTED),
                        );
                    });
                });
            });
        egui::SidePanel::left("navigation")
            .show_separator_line(false)
            .exact_width(if wide { 232.0 } else { 208.0 })
            .resizable(false)
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 18,
                right: 8,
                top: 10,
                bottom: 16,
            }))
            .show(ctx, |ui| {
                let h = (ui.available_height() - 28.0).max(400.0);
                crate::design::card()
                    .inner_margin(14)
                    .show(ui, |ui| self.navigation(ui, h));
            });
        if wide {
            egui::SidePanel::right("phone-summary")
                .show_separator_line(false)
                .exact_width(350.0)
                .resizable(false)
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    left: 8,
                    right: 18,
                    top: 10,
                    bottom: 16,
                }))
                .show(ctx, |ui| {
                    let h = (ui.available_height() - 32.0).max(400.0);
                    crate::design::card().inner_margin(16).show(ui, |ui| {
                        ui.set_min_height(h);
                        egui::ScrollArea::vertical()
                            .id_salt("phone-summary-scroll")
                            .max_height(h)
                            .show(ui, |ui| self.summary(ui));
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 8,
                right: if wide { 8 } else { 18 },
                top: 10,
                bottom: 16,
            }))
            .show(ctx, |ui| {
                let h = (ui.available_height() - 40.0).max(400.0);
                self.content_height = h;
                crate::design::card().show(ui, |ui| {
                    ui.set_min_height(h);
                    egui::ScrollArea::vertical()
                        .id_salt("main-content")
                        .max_height(h)
                        .show(ui, |ui| {
                            if self.initializing.is_some() {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label("正在准备应用…");
                                });
                            }
                            if let Some(e) = self.init_error.clone() {
                                ui.colored_label(ui.visuals().error_fg_color, e);
                                if ui.button("重试启动").clicked() {
                                    self.initialize();
                                }
                            }
                            if !self.search.trim().is_empty() {
                                self.search_results(ui);
                            } else {
                                match self.tab {
                                    2 => self.details(ui),
                                    3 => self.tool_page(ui, "备份与刷机"),
                                    4 => self.tool_page(ui, "应用与文件"),
                                    5 => self.tool_page(ui, "解锁与配置"),
                                    6 => self.tool_page(ui, "安卓设备"),
                                    _ => self.overview(ui),
                                }
                            }
                            if !wide && self.tab == 1 && !self.show_common {
                                ui.add_space(24.0);
                                ui.separator();
                                ui.heading("设备详情");
                                self.summary(ui);
                            }
                            if self.tab != 1 || self.show_common || !self.last_error.is_empty() {
                                ui.add_space(18.0);
                                ui.collapsing("运行记录", |ui| {
                                    if ui.button("保存运行记录").clicked() {
                                        if let Some(path) = rfd::FileDialog::new()
                                            .set_file_name("Unbound.log")
                                            .save_file()
                                        {
                                            let text: String =
                                                self.logs.iter().map(|(_, s)| s.as_str()).collect();
                                            if let Err(e) = std::fs::write(path, text) {
                                                self.last_error = e.to_string();
                                            }
                                        }
                                    }
                                    egui::ScrollArea::vertical()
                                        .max_height(200.0)
                                        .stick_to_bottom(true)
                                        .show(ui, |ui| {
                                            for (err, s) in &self.logs {
                                                if *err {
                                                    ui.colored_label(
                                                        ui.visuals().error_fg_color,
                                                        s,
                                                    );
                                                } else {
                                                    ui.label(s);
                                                }
                                            }
                                        });
                                });
                            }
                        });
                });
            });
        self.help_windows(ctx);
        // 使用时长累计，到点弹一次支持提示（关掉后本会话不再弹）。
        self.usage_seconds += ctx.input(|i| i.stable_dt).max(0.0);
        if crate::support::should_prompt(
            self.usage_seconds,
            self.support_dismissed,
            self.support_shown,
        ) {
            self.support_shown = true;
            self.support_open = true;
        }
        self.support_window(ctx);
    }
}

fn install_font(ctx: &egui::Context) {
    let mut paths = Vec::new();
    #[cfg(windows)]
    paths.push(
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into()))
            .join("Fonts/msyh.ttc"),
    );
    #[cfg(target_os = "macos")]
    paths.push(PathBuf::from("/System/Library/Fonts/PingFang.ttc"));
    for path in paths {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("chinese".into(), egui::FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, "chinese".into());
            }
            ctx.set_fonts(fonts);
            break;
        }
    }
}
