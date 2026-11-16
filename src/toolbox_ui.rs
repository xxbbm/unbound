use apple_toolkit::{
    actions::{self, Input, Task},
    mdm::Device,
};
use eframe::egui;
use std::path::PathBuf;

pub struct Center {
    pub selected: String,
    input: String,
    output: String,
    enabled: bool,
    full: bool,
    password: String,
    password_new: String,
    confirm: String,
    group: String,
    last_selected: String,
    pub entries: Vec<actions::native::Entry>,
    pub message: String,
    pub task_path: Option<PathBuf>,
    entries_crashes: bool,
    apps: Vec<actions::native::AppEntry>,
    metrics: Option<actions::native::MetricSample>,
}
impl Default for Center {
    fn default() -> Self {
        Self {
            selected: "apps".into(),
            input: String::new(),
            output: String::new(),
            enabled: false,
            full: false,
            password: String::new(),
            password_new: String::new(),
            confirm: String::new(),
            group: "全部".into(),
            last_selected: String::new(),
            entries: vec![],
            message: String::new(),
            task_path: None,
            entries_crashes: false,
            apps: vec![],
            metrics: None,
        }
    }
}
impl Center {
    pub fn choose_group(&mut self, group: &str) {
        if self.group != group {
            self.group = group.into();
            self.selected = match group {
                "备份与刷机" => "backup",
                "应用与文件" => "apps",
                "解锁与配置" => "bypass-prepare",
                "安卓设备" => "android-devices",
                _ => "device-monitor",
            }
            .into();
        }
    }
    pub fn stop(&mut self, run: &apple_toolkit::runner::Run) {
        run.cancel();
    }
    pub fn finished(&mut self) {
        if self.selected == "device-monitor" && self.metrics.is_some() {
            self.message = "监控已停止，以下保留最后一次读数。".into();
        }
        if let Some(p) = self.task_path.take() {
            let _ = std::fs::remove_file(p);
        }
    }
    pub fn clear_device(&mut self) {
        self.metrics = None;
        self.confirm.clear();
        self.entries.clear();
        self.apps.clear();
        self.password.clear();
        self.password_new.clear();
        self.message.clear();
    }
    pub fn protocol(&mut self, line: &str) {
        if let Some(s) = line.strip_prefix("[TASK_RESULT_JSON] ") {
            if let Ok(message) = serde_json::from_str::<String>(s) {
                self.message = message;
                self.confirm.clear();
            }
        }
        if let Some(s) = line.strip_prefix("[METRICS] ") {
            if let Ok(sample) = serde_json::from_str(s) {
                self.metrics = Some(sample);
                self.message = "正在读取，每 2 秒刷新一次。".into();
            }
        }
        if let Some(s) = line.strip_prefix("[APPS] ") {
            if let Ok(v) = serde_json::from_str(s) {
                self.apps = v;
            }
        }
        if let Some(s) = line.strip_prefix("[FILES] ") {
            if let Ok(v) = serde_json::from_str(s) {
                self.entries = v;
                self.entries_crashes = self.selected.starts_with("crash");
            }
        }
        if let Some(s) = line.strip_prefix("[TASK_RESULT] ") {
            self.message = s.into();
            self.confirm.clear();
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, device: Option<&Device>, busy: bool) -> Option<Task> {
        let mut task = None;
        ui.heading(&self.group);
        ui.small("选择下方功能，按右侧提示操作。");
        ui.separator();
        let all = actions::catalog();
        let actions: Vec<_> = all
            .iter()
            .filter(|a| a.group == self.group)
            .copied()
            .collect();
        if actions.is_empty() {
            ui.label("此分类暂时没有可用项目。");
            return None;
        }
        if !busy && !actions.iter().any(|a| a.id == self.selected) {
            self.selected = actions[0].id.into();
        }
        ui.horizontal_top(|ui|{
   ui.allocate_ui_with_layout(egui::vec2(180.0,ui.available_height().min(330.0)),egui::Layout::top_down(egui::Align::Min),|ui|{
    egui::ScrollArea::vertical().id_salt("tool-list").max_height(310.0).show(ui,|ui|{for a in &actions {if ui.add_enabled(!busy,egui::Button::new(a.name).selected(self.selected==a.id).corner_radius(12).min_size(egui::vec2(176.0,40.0))).clicked(){self.selected=a.id.into();}}});
   });
   ui.add_sized([8.0,330.0],egui::Separator::default().vertical());
   ui.vertical(|ui|{
    let Some(a)=all.iter().find(|a|a.id==self.selected).copied()else{return};
    if self.last_selected!=self.selected {self.last_selected=self.selected.clone();self.input=if ["files","crashes"].contains(&a.id){"/".into()}else{String::new()};self.output.clear();self.enabled=false;self.confirm.clear();self.password.clear();self.password_new.clear();self.message.clear();self.entries.clear();self.metrics=None;}
    ui.heading(a.name);ui.label(a.help);ui.add_space(12.0);
    if actions::local::is_local(a.id){ui.label(if a.id=="recovery-scan"{"目标：USB 恢复/DFU 设备（无需正常开机连接）"}else{"目标：这台电脑（无需连接手机）"});}else if let Some(d)=device{ui.strong(format!("目标：{} · 序列号 {}",apple_toolkit::models::name(&d.product),d.serial));}else{ui.colored_label(ui.visuals().warn_fg_color,"请先连接并解锁手机，接受“信任此电脑”。");}
    ui.add_enabled_ui(!busy,|ui|{
     match a.input{
      Input::File|Input::Directory=>{ui.label(a.label);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.input).desired_width(235.0));if ui.button("选择…").clicked(){let chosen=if a.input==Input::Directory{rfd::FileDialog::new().pick_folder()}else{let dialog=rfd::FileDialog::new();let dialog=match a.id{"flash"|"firmware-info"=>dialog.add_filter("iOS 固件",&["ipsw"]),"install"=>dialog.add_filter("iOS 应用",&["ipa"]),_=>dialog};dialog.pick_file()};if let Some(p)=chosen{self.input=p.to_string_lossy().into_owned();}}});},
      Input::Output=>{ui.label(a.label);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.output).desired_width(235.0));if ui.button("另存为…").clicked(){let name=if a.id=="screenshot"{"手机截图.png"}else{"设备配置.plist"};if let Some(p)=rfd::FileDialog::new().set_file_name(name).save_file(){self.output=p.to_string_lossy().into_owned();}}});},
      Input::Text=>{ui.label(a.label);ui.add(egui::TextEdit::singleline(&mut self.input).desired_width(310.0));},
      Input::BackupPassword=>{if a.id=="backup-password"{ui.checkbox(&mut self.enabled,"开启备份加密（取消勾选表示关闭加密）");ui.label("开启时填写新密码，关闭时填写当前密码");}else{ui.label("当前备份密码");}ui.add(egui::TextEdit::singleline(&mut self.password).password(true).desired_width(280.0));},Input::None=>()
     }
     if a.id=="backup-change-password"{ui.label("新备份密码");ui.add(egui::TextEdit::singleline(&mut self.password_new).password(true));}
     if a.id=="backup-copy"{ui.label("复制目标（选择另一块磁盘上的文件夹）");ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.output);if ui.button("选择目标…").clicked(){if let Some(p)=rfd::FileDialog::new().pick_folder(){self.output=p.to_string_lossy().into_owned();}}});ui.small("复制完成并校验前请勿运行 iTunes/Apple Devices 的备份任务。不会删除原备份，也不会更改系统链接。");}
     if a.id=="backup"{ui.checkbox(&mut self.full,"完整备份（取消勾选则允许增量备份）");}
     if a.id=="restore-backup"{ui.label("备份密码（未加密留空）");ui.add(egui::TextEdit::singleline(&mut self.password).password(true));}
     if a.id=="flash"{ui.checkbox(&mut self.enabled,"尝试保资料更新（仍可能丢失资料；默认清空刷机）");ui.small("只支持正常模式选定的设备；会先核对固件机型及 ECID，再进入刷机。此操作需要手机正常开机并已信任电脑。");}
     if a.dangerous {ui.add_space(10.0);ui.group(|ui|{ui.colored_label(ui.visuals().warn_fg_color,"此操作会修改手机。请确认已经备份需要的资料。");ui.label("输入上方手机序列号以确认本次操作：");ui.add(egui::TextEdit::singleline(&mut self.confirm).desired_width(240.0));});}
     ui.add_space(12.0);
     let ready=(device.is_some() || actions::local::is_local(a.id))&&(!a.dangerous||device.is_some_and(|d|self.confirm==d.serial));
     if ui.add_enabled(ready,egui::Button::new(egui::RichText::new(format!("运行：{}",a.name)).color(egui::Color32::WHITE)).fill(crate::design::BLUE).stroke(egui::Stroke::NONE).min_size(egui::vec2(240.0,44.0))).clicked(){
      if ["file-download","crash-download"].contains(&a.id){if let Some(p)=rfd::FileDialog::new().set_file_name(self.input.rsplit('/').next().unwrap_or("手机文件")).save_file(){self.output=p.to_string_lossy().into_owned();}else{return}}
      if let Some(d)=device.cloned().or_else(||if actions::local::is_local(a.id){Some(Device{udid:String::new(),serial:String::new(),product:String::new(),version:String::new(),build:String::new(),name:String::new(),imei:String::new(),activation:String::new(),facts:Default::default()})}else{None}) {let t=Task{id:a.id.into(),device:d.clone(),input:self.input.clone(),output:self.output.clone(),enabled:self.enabled,full:self.full,password:self.password.clone(),password_new:self.password_new.clone(),confirm:self.confirm.clone(),stop_file:String::new()};match t.validate(){Ok(_)=>{self.message="正在启动…".into();self.metrics=None;self.confirm.clear();self.password.clear();self.password_new.clear();task=Some(t)},Err(e)=>self.message=e}}
     }
    });
    if !self.message.is_empty(){ui.add_space(10.0);ui.label(&self.message);}
   });
  });
        if self.selected == "device-monitor" {
            if let Some(sample) = &self.metrics {
                ui.separator();
                ui.strong(if busy {
                    "实时设备读数（2 秒一次）"
                } else {
                    "最后一次设备读数（已停止）"
                });
                let read_at = time::OffsetDateTime::from_unix_timestamp(sample.read_at as i64)
                    .ok()
                    .map(|t| {
                        t.to_offset(
                            time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC),
                        )
                    })
                    .and_then(|t| {
                        t.format(&time::format_description::well_known::Rfc3339)
                            .ok()
                    })
                    .unwrap_or_else(|| "时间不可用".into());
                ui.small(format!("最后更新：{}", read_at.replace('T', " ")));
                egui::Grid::new("device-metrics")
                    .striped(true)
                    .show(ui, |ui| {
                        for (key, value) in &sample.values {
                            if let Some(label) = key.strip_suffix("（字节）") {
                                ui.label(label);
                                let display = value
                                    .parse::<u64>()
                                    .map(|n| format!("{:.2} GB", n as f64 / 1_000_000_000.0))
                                    .unwrap_or_else(|_| value.clone());
                                ui.label(display)
                                    .on_hover_text(format!("设备原始读数：{value} 字节"));
                            } else {
                                ui.label(key);
                                ui.label(match value.as_str() {
                                    "true" => "是",
                                    "false" => "否",
                                    _ => value,
                                });
                            }
                            ui.end_row();
                        }
                    });
                for error in &sample.errors {
                    ui.small(error);
                }
            }
        }
        if !self.entries.is_empty() {
            ui.separator();
            ui.strong("手机文件（点击目录进入；选择文件后可下载或删除）");
            let mut pick = None;
            egui::ScrollArea::vertical()
                .id_salt("phone-files")
                .max_height(260.0)
                .show(ui, |ui| {
                    for entry in &self.entries {
                        ui.horizontal(|ui| {
                            if ui
                                .button(format!(
                                    "{} {}",
                                    if entry.directory { "目录" } else { "文件" },
                                    entry.name
                                ))
                                .clicked()
                            {
                                pick = Some((
                                    entry.path.clone(),
                                    if self.entries_crashes {
                                        if entry.directory {
                                            "crashes"
                                        } else {
                                            "crash-download"
                                        }
                                    } else if entry.directory {
                                        "files"
                                    } else {
                                        "file-download"
                                    },
                                ));
                            }
                            ui.small(format!("{} 字节", entry.size));
                            if !entry.directory
                                && ui
                                    .add_enabled(!busy, egui::Button::new("删除").small())
                                    .clicked()
                            {
                                pick = Some((
                                    entry.path.clone(),
                                    if self.entries_crashes {
                                        "crash-remove"
                                    } else {
                                        "file-delete"
                                    },
                                ));
                            }
                        });
                    }
                });
            if let Some((path, id)) = pick {
                self.selected = id.into();
                self.last_selected = id.into();
                self.input = path;
                self.confirm.clear();
            }
        }
        if self.selected == "apps" && !self.apps.is_empty() {
            ui.separator();
            ui.strong("应用列表");
            let mut pick = None;
            egui::ScrollArea::vertical()
                .id_salt("app-table")
                .max_height(260.0)
                .show(ui, |ui| {
                    for app in &self.apps {
                        ui.horizontal(|ui| {
                            ui.label(&app.name);
                            ui.small(&app.version);
                            ui.small(&app.identifier);
                            if ui.add_enabled(!busy, egui::Button::new("卸载…")).clicked() {
                                pick = Some(app.identifier.clone());
                            }
                        });
                    }
                });
            if let Some(id) = pick {
                self.selected = "uninstall".into();
                self.last_selected = "uninstall".into();
                self.input = id;
                self.confirm.clear();
            }
        }
        task
    }
}
