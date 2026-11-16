//! Reference-led iOS-style presentation; all rows are backed by live state.
use super::*;
use crate::design::{self as d, Icon};
use egui::{Color32, RichText, Stroke};

impl ToolkitApp {
    /// 标题下方那行说明：显示当前生效的平台与选择方式。
    pub(super) fn platform_label(&self) -> String {
        use apple_toolkit::platform::{resolve, Platform, Selection};
        let android_usable = self
            .android_devices
            .iter()
            .filter(|d| d.state.usable())
            .count();
        let ios_count = if self.device.is_some() { 1 } else { 0 };
        let active = resolve(self.platform, ios_count, android_usable);
        match (self.platform, active) {
            (Selection::Auto, Some(Platform::Ios)) => "自动识别：已连接 iOS 设备".into(),
            (Selection::Auto, Some(Platform::Android)) => "自动识别：已连接 Android 设备".into(),
            (Selection::Auto, None) => "自动识别：等待设备连接".into(),
            (Selection::Fixed(platform), Some(_)) => {
                format!("已固定为 {} 设备管理", platform.label())
            }
            (Selection::Fixed(platform), None) => {
                format!("已固定为 {}，但当前未检测到设备", platform.label())
            }
        }
    }

    pub(super) fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 52.0), egui::Sense::hover());
            d::logo(
                ui.painter(),
                egui::Rect::from_center_size(r.center(), egui::vec2(48.0, 48.0)),
                &self.brand,
            );
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Unbound").size(30.0).strong());
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new("闲鱼@学生党免费")
                            .size(13.0)
                            .color(d::MUTED),
                    );
                });
                // 标题下方：平台切换（自动 / iOS / Android）。
                // 自动模式下由检测结果决定；手动选择优先，便于安卓机插着不动时
                // 仍能切回 iOS 处理。
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (label, selection) in [
                        ("自动", apple_toolkit::platform::Selection::Auto),
                        ("iOS", apple_toolkit::platform::Selection::Fixed(apple_toolkit::platform::Platform::Ios)),
                        ("Android", apple_toolkit::platform::Selection::Fixed(apple_toolkit::platform::Platform::Android)),
                    ] {
                        let active = self.platform == selection;
                        let accent = Color32::from_rgb(37, 99, 235);
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label)
                                        .size(12.0)
                                        .color(if active { Color32::WHITE } else { d::MUTED }),
                                )
                                .fill(if active { accent } else { Color32::TRANSPARENT })
                                .stroke(Stroke::new(1.0_f32, if active { accent } else { d::BORDER }))
                                .corner_radius(10)
                                .min_size(egui::vec2(0.0, 20.0)),
                            )
                            .clicked()
                        {
                            self.platform = selection;
                        }
                    }
                });
                ui.label(
                    RichText::new(self.platform_label())
                        .color(d::MUTED)
                        .size(13.0),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized([100.0, 44.0], egui::Button::new("使用帮助"))
                    .clicked()
                {
                    self.show_help = true;
                }
                let width = (ui.available_width() - 20.0).clamp(220.0, 330.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(width + 20.0, 46.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(248, 249, 252))
                            .stroke(Stroke::new(1.0_f32, d::BORDER))
                            .corner_radius(16)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                ui.spacing_mut().interact_size.y = 26.0;
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(22.0, 24.0),
                                            egui::Sense::hover(),
                                        );
                                        d::icon(
                                            ui.painter(),
                                            r.center(),
                                            21.0,
                                            Icon::Search,
                                            d::MUTED,
                                        );
                                        let response = ui.add(
                                            egui::TextEdit::singleline(&mut self.search)
                                                .hint_text(
                                                    RichText::new("搜索设备、功能…")
                                                        .color(d::MUTED),
                                                )
                                                .frame(false)
                                                .desired_width(width - 44.0),
                                        );
                                        if ui.input(|i| {
                                            i.modifiers.command && i.key_pressed(egui::Key::K)
                                        }) {
                                            response.request_focus();
                                        }
                                    },
                                );
                            });
                    },
                );
            });
        });
    }
    pub(super) fn navigation(&mut self, ui: &mut egui::Ui, height: f32) {
        ui.set_min_height(height);
        ui.add_space(3.0);
        for (id, label, icon) in [
            (1, "设备总览", Icon::Home),
            (2, "详细验机", Icon::Info),
            (3, "备份与刷机", Icon::Backup),
            (4, "应用与文件", Icon::Folder),
            (5, "解锁与配置", Icon::Check),
            (6, "安卓设备", Icon::Phone),
        ] {
            if id == 2 {
                ui.add_space(16.0);
                ui.label(RichText::new("管理").size(13.0).color(d::MUTED));
                ui.add_space(4.0);
            }
            if id == 5 {
                ui.add_space(16.0);
                ui.label(RichText::new("解锁与安卓").size(13.0).color(d::MUTED));
                ui.add_space(4.0);
            }
            if d::nav(
                ui,
                icon,
                label,
                self.tab == id,
                self.job != Some(Job::Other),
            )
            .clicked()
            {
                self.tab = id;
                self.search.clear();
                self.show_common = false;
            }
        }
        ui.add_space(24.0);
        ui.label(RichText::new("支持").size(13.0).color(d::MUTED));
        ui.add_space(4.0);
        if d::nav(ui, Icon::Help, "连接帮助", false, true).clicked() {
            self.show_help = true;
        }
        if d::nav(ui, Icon::Info, "关于 Unbound", false, true).clicked() {
            self.show_about = true;
        }
        let gap = (ui.available_height() - 102.0).max(16.0);
        ui.add_space(gap);
        egui::Frame::new()
            .fill(Color32::from_white_alpha(190))
            .stroke(Stroke::new(1.0_f32, Color32::WHITE))
            .corner_radius(16)
            .inner_margin(13)
            .show(ui, |ui| {
                d::status(
                    ui,
                    self.device.is_some(),
                    if self.device.is_some() {
                        "设备已连接"
                    } else {
                        "等待连接"
                    },
                );
                ui.label(
                    RichText::new(if self.device.is_some() {
                        "设备信息已读取"
                    } else {
                        "连接并解锁你的 iPhone"
                    })
                    .size(12.0)
                    .color(d::MUTED),
                );
            });
    }
    pub(super) fn select_action(&mut self, id: &str) {
        if let Some(action) = apple_toolkit::actions::catalog()
            .iter()
            .find(|a| a.id == id)
        {
            self.tab = match action.group {
                "备份与刷机" => 3,
                "应用与文件" => 4,
                "解锁与配置" => 5,
                "安卓设备" => 6,
                _ => 1,
            };
            self.toolbox.choose_group(action.group);
            self.toolbox.selected = id.into();
            self.show_common = self.tab == 1;
            self.search.clear();
        }
    }
    /// 当前生效的平台：自动模式看检测结果，固定模式看用户选择。
    pub(super) fn active_platform(&self) -> Option<apple_toolkit::platform::Platform> {
        let android_usable = self
            .android_devices
            .iter()
            .filter(|d| d.state.usable())
            .count();
        let ios = if self.device.is_some() { 1 } else { 0 };
        apple_toolkit::platform::resolve(self.platform, ios, android_usable)
    }

    /// 第一台可用安卓设备的序列号。
    pub(super) fn android_serial(&self) -> Option<String> {
        self.android_devices
            .iter()
            .find(|d| d.state.usable())
            .map(|d| d.serial.clone())
    }

    /// 安卓平台的设备总览。与 iOS 那套完全分开，避免动到既有布局。
    pub(super) fn android_overview(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("安卓设备");
                ui.label(
                    RichText::new("通过 adb 连接；请在手机上允许 USB 调试").color(d::MUTED),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized([132.0, 42.0], egui::Button::new("刷新设备信息"))
                    .clicked()
                {
                    self.android_status = None;
                    self.android_probe.cancel();
                }
            });
        });
        ui.add_space(16.0);

        if self.android_devices.is_empty() {
            ui.label(
                RichText::new("未检测到安卓设备。用数据线连接并允许 USB 调试后会自动出现。")
                    .color(d::MUTED),
            );
            return;
        }

        // 设备列表：序列号 + 授权状态 + 机型
        for device in &self.android_devices {
            let usable = device.state.usable();
            ui.horizontal(|ui| {
                ui.label(RichText::new(&device.serial).strong());
                ui.label(RichText::new(device.state.label()).color(if usable {
                    d::MUTED
                } else {
                    Color32::from_rgb(180, 83, 9)
                }));
                if let Some(model) = &device.model {
                    ui.label(RichText::new(model).color(d::MUTED));
                }
            });
        }
        ui.add_space(14.0);

        let Some(status) = &self.android_status else {
            ui.label(RichText::new("正在读取设备信息…").color(d::MUTED));
            return;
        };

        // 基本信息：能拿到几项就显示几项，拿不到的写"未获取"
        egui::Frame::new()
            .fill(Color32::from_rgb(248, 249, 252))
            .stroke(Stroke::new(1.0_f32, d::BORDER))
            .corner_radius(14)
            .inner_margin(14)
            .show(ui, |ui| {
                let system = status.info.version.as_ref().map(|v| match &status.info.sdk {
                    Some(sdk) => format!("Android {v}（API {sdk}）"),
                    None => format!("Android {v}"),
                });
                let rows = [
                    ("机型", status.info.model.clone()),
                    ("系统", system),
                    ("架构", status.info.abi.clone()),
                    ("序列号", status.info.serial.clone()),
                    ("电池", Some(status.battery.summary())),
                    ("存储", Some(status.storage.summary())),
                    ("屏幕", Some(status.screen.summary())),
                ];
                for (label, value) in rows {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [72.0, 20.0],
                            egui::Label::new(RichText::new(label).color(d::MUTED)),
                        );
                        ui.label(match value {
                            Some(text) if !text.trim().is_empty() => text,
                            _ => "未获取".to_string(),
                        });
                    });
                }
            });
    }

    /// 支持提示弹窗。20 分钟使用后自动弹一次，也可以从「关于」手动打开。
    pub(super) fn support_window(&mut self, ctx: &egui::Context) {
        if !self.support_open {
            return;
        }
        if self.support_tex.is_none() {
            self.support_tex = crate::design::load_support(ctx);
        }
        let Some((reward, alipay)) = self.support_tex.clone() else {
            // 二维码解不出来就别弹了，不打扰用户
            self.support_open = false;
            return;
        };

        let mut close = false;
        let mut never = false;
        let mut save_result: Option<Result<std::path::PathBuf, String>> = None;
        let mut request_zoom: Option<u8> = None;
        egui::Window::new("支持 Unbound")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("这个工具帮你省了时间？可以请作者喝杯咖啡 ☕")
                        .size(15.0),
                );
                ui.label(
                    RichText::new("扫码即可，金额随意，不影响任何功能。")
                        .size(12.0)
                        .color(d::MUTED),
                );
                ui.add_space(14.0);
                let mut zoom_request: Option<u8> = None;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("微信赞赏").strong());
                        ui.add_space(6.0);
                        let response = ui.add(
                            egui::Image::new((reward.id(), egui::vec2(250.0, 250.0)))
                                .sense(egui::Sense::click()),
                        );
                        if response.clicked() {
                            zoom_request = Some(0);
                        }
                        response.on_hover_text("点击放大");
                        ui.label(
                            RichText::new("点击图片放大")
                                .size(11.0)
                                .color(d::MUTED),
                        );
                    });
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("支付宝").strong());
                        ui.add_space(6.0);
                        let response = ui.add(
                            egui::Image::new((alipay.id(), egui::vec2(250.0, 250.0)))
                                .sense(egui::Sense::click()),
                        );
                        if response.clicked() {
                            zoom_request = Some(1);
                        }
                        response.on_hover_text("点击放大");
                        ui.label(
                            RichText::new("点击图片放大")
                                .size(11.0)
                                .color(d::MUTED),
                        );
                    });
                });
                if zoom_request.is_some() {
                    request_zoom = zoom_request;
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new("微信赞赏码请用微信「扫一扫」→ 右上角相册，选中图片识别。")
                        .size(12.0)
                        .color(d::MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("保存原图到桌面").clicked() {
                        save_result = Some(crate::support::save_images());
                    }
                    ui.label(
                        RichText::new("扫不上就用原图：保存后用手机相册识别")
                            .size(12.0)
                            .color(d::MUTED),
                    );
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("稍后再说").clicked() {
                        close = true;
                    }
                    if ui.button("我支持过了，不再提示").clicked() {
                        never = true;
                    }
                });
            });

        if let Some(result) = save_result {
            self.notice = match result {
                Ok(dir) => format!("支持码已保存到 {}", dir.display()),
                Err(e) => e,
            };
        }
        if let Some(index) = request_zoom {
            self.support_zoom = Some(index);
        }

        // 放大查看：按屏幕高度取尺寸，尽量大到能直接扫
        if let Some(index) = self.support_zoom {
            let (texture, label) = if index == 0 {
                (&reward, "微信赞赏码")
            } else {
                (&alipay, "支付宝收款码")
            };
            let mut close_zoom = false;
            egui::Window::new(label)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let side = (ctx.screen_rect().height() * 0.72).clamp(320.0, 760.0);
                    ui.image((texture.id(), egui::vec2(side, side)));
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("扫不上就用「保存原图到桌面」，再从手机相册识别")
                            .size(12.0)
                            .color(d::MUTED),
                    );
                    ui.add_space(6.0);
                    if ui.button("关闭").clicked() {
                        close_zoom = true;
                    }
                });
            if close_zoom {
                self.support_zoom = None;
            }
        }

        if close {
            self.support_open = false;
        }
        if never {
            self.support_open = false;
            self.support_dismissed = true;
            crate::support::set_dismissed();
        }
    }

    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        // 安卓平台走另一套总览：iOS 那套依赖 ideviceinfo 的字段。
        if self.active_platform() == Some(apple_toolkit::platform::Platform::Android) {
            self.android_overview(ui);
            return;
        }
        if self.show_common {
            if ui.button("返回设备总览").clicked() && self.run.is_none() {
                self.show_common = false;
            }
            ui.add_space(12.0);
            self.tool_page(ui, "常用操作");
            return;
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("设备操作");
                ui.label(RichText::new("选择设备并执行管理操作").color(d::MUTED));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let refresh = ui.add_enabled(
                    self.run.is_none(),
                    egui::Button::new("    刷新设备列表").min_size(egui::vec2(152.0, 42.0)),
                );
                d::icon(
                    ui.painter(),
                    egui::pos2(refresh.rect.left() + 20.0, refresh.rect.center().y),
                    19.0,
                    Icon::Refresh,
                    d::TEXT,
                );
                if refresh.clicked() {
                    self.start_mdm(Job::Probe, None);
                }
            });
        });
        ui.add_space(20.0);
        search_field(
            ui,
            &mut self.device_search,
            "搜索设备名称、型号或 UDID",
            ui.available_width(),
        );
        ui.add_space(16.0);
        let table_height = (self.content_height - 398.0).clamp(170.0, 320.0);
        let width = ui.available_width();
        egui::Frame::new()
            .fill(Color32::from_white_alpha(155))
            .stroke(Stroke::new(1.0_f32, Color32::WHITE))
            .corner_radius(17)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_width(width - 24.0);
                ui.set_min_height(table_height);
                let (head, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 30.0),
                    egui::Sense::hover(),
                );
                for (x, label) in [
                    (0.025, "设备名称"),
                    (0.40, "型号"),
                    (0.71, "iOS"),
                    (0.87, "状态"),
                ] {
                    ui.painter().text(
                        egui::pos2(head.left() + head.width() * x, head.center().y),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(13.0),
                        d::MUTED,
                    );
                }
                ui.add_space(8.0);
                if let Some(device) = self.device.clone() {
                    let filter = self.device_search.to_lowercase();
                    let haystack = format!(
                        "{} {} {} {}",
                        device.name,
                        device.product,
                        apple_toolkit::models::name(&device.product),
                        device.udid
                    )
                    .to_lowercase();
                    if filter.is_empty() || haystack.contains(&filter) {
                        let response = ui.add_sized(
                            [ui.available_width(), 74.0],
                            egui::Button::new("")
                                .fill(d::SELECTED)
                                .stroke(Stroke::NONE)
                                .corner_radius(12),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                format!(
                                    "{}，已连接，查看设备信息",
                                    apple_toolkit::models::name(&device.product)
                                ),
                            )
                        });
                        let r = response.rect;
                        let p = ui.painter();
                        let cy = r.center().y;
                        p.circle_filled(egui::pos2(r.left() + 18.0, cy), 10.0, d::BLUE);
                        d::icon(
                            p,
                            egui::pos2(r.left() + 18.0, cy),
                            14.0,
                            Icon::Check,
                            Color32::WHITE,
                        );
                        let columns = [
                            (0.075, 0.39, device.name.as_str()),
                            (0.40, 0.70, apple_toolkit::models::name(&device.product)),
                            (0.71, 0.85, device.version.as_str()),
                        ];
                        for (left, right, text) in columns {
                            let clip = egui::Rect::from_min_max(
                                egui::pos2(r.left() + r.width() * left, r.top()),
                                egui::pos2(r.left() + r.width() * right, r.bottom()),
                            );
                            let center_y = if left < 0.1 { cy - 10.0 } else { cy };
                            p.with_clip_rect(clip).text(
                                egui::pos2(clip.left(), center_y),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::proportional(14.0),
                                d::TEXT,
                            );
                        }
                        p.text(
                            egui::pos2(r.left() + r.width() * 0.075, cy + 13.0),
                            egui::Align2::LEFT_CENTER,
                            "USB 已连接",
                            egui::FontId::proportional(12.0),
                            d::MUTED,
                        );
                        p.circle_filled(
                            egui::pos2(r.left() + r.width() * 0.875, cy),
                            4.0,
                            d::GREEN,
                        );
                        p.text(
                            egui::pos2(r.left() + r.width() * 0.895, cy),
                            egui::Align2::LEFT_CENTER,
                            "在线",
                            egui::FontId::proportional(13.0),
                            d::TEXT,
                        );
                        if response.on_hover_text("点击读取设备详细信息").clicked()
                            && self.run.is_none()
                        {
                            self.read_details();
                        }
                    } else {
                        ui.add_space(30.0);
                        ui.label("没有匹配的设备，请换一个搜索词。");
                    }
                } else {
                    ui.add_space(26.0);
                    ui.vertical_centered(|ui| {
                        let (r, _) =
                            ui.allocate_exact_size(egui::vec2(62.0, 62.0), egui::Sense::hover());
                        d::icon(ui.painter(), r.center(), 43.0, Icon::Usb, d::MUTED);
                        ui.label(RichText::new("连接你的 iPhone").size(19.0).strong());
                        ui.label(
                            RichText::new("用数据线连接，解锁并信任此电脑")
                                .size(14.0)
                                .color(d::MUTED),
                        );
                        ui.add_space(10.0);
                        if ui.button("查看连接指引").clicked() {
                            self.show_help = true;
                        }
                    });
                }
            });
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!("共 {} 台设备", usize::from(self.device.is_some())))
                .size(13.0)
                .color(d::MUTED),
        );
        ui.add_space(18.0);
        let tile_width = ((ui.available_width() - 32.0) / 5.0).max(70.0);
        let connected = self.device.is_some() && self.run.is_none();
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for (kind, label, id) in [
                (Icon::Info, "设备信息", "details"),
                (Icon::Backup, "备份设备", "backup"),
                (Icon::Trash, "抹除设备", "erase"),
                (Icon::Restart, "重启设备", "reboot"),
                (Icon::More, "更多操作", "more"),
            ] {
                if d::tile(
                    ui,
                    kind,
                    label,
                    tile_width,
                    if id == "more" {
                        self.run.is_none()
                    } else {
                        connected
                    },
                )
                .clicked()
                {
                    action = Some(id);
                }
            }
        });
        match action {
            Some("details") => self.read_details(),
            Some("more") => self.show_common = true,
            Some(id) => self.select_action(id),
            None => (),
        }
    }
    pub(super) fn search_results(&mut self, ui: &mut egui::Ui) {
        ui.heading("搜索结果");
        ui.label(RichText::new(format!("“{}”", self.search)).color(d::MUTED));
        ui.add_space(18.0);
        let query = self.search.to_lowercase();
        let mut found = false;
        let mut chosen = None;
        if let Some(device) = &self.device {
            if format!(
                "{} {} {} {}",
                device.name, device.product, device.udid, device.serial
            )
            .to_lowercase()
            .contains(&query)
            {
                found = true;
                if ui
                    .add_sized(
                        [ui.available_width(), 50.0],
                        egui::Button::new(format!(
                            "{} · 查看设备信息",
                            apple_toolkit::models::name(&device.product)
                        )),
                    )
                    .clicked()
                    && self.run.is_none()
                {
                    self.search.clear();
                    self.read_details();
                    return;
                }
            }
        }
        for action in apple_toolkit::actions::catalog() {
            if format!("{} {}", action.name, action.group)
                .to_lowercase()
                .contains(&query)
            {
                found = true;
                if ui
                    .add_enabled(
                        self.run.is_none(),
                        egui::Button::new(format!("{}    ·    {}", action.name, action.group))
                            .min_size(egui::vec2(ui.available_width(), 48.0)),
                    )
                    .clicked()
                {
                    chosen = Some(action.id);
                }
            }
        }
        if let Some(id) = chosen {
            self.select_action(id);
        }
        if !found {
            ui.label("没有匹配的设备或功能。");
        }
        ui.add_space(16.0);
        if ui.button("清除搜索").clicked() {
            self.search.clear();
        }
    }
    pub(super) fn summary(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().interact_size.y = 24.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        let Some(device) = self.device.clone() else {
            let (r, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 170.0),
                egui::Sense::hover(),
            );
            d::wash(
                ui.painter(),
                r,
                Color32::from_rgb(225, 225, 239),
                Color32::from_rgb(245, 246, 252),
            );
            d::icon(
                ui.painter(),
                r.center(),
                102.0,
                Icon::Phone,
                Color32::from_rgb(148, 153, 175),
            );
            ui.add_space(18.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("尚未连接设备").size(21.0).strong());
                ui.add_space(5.0);
                ui.label(RichText::new("连接后自动显示机型与参数").color(d::MUTED));
            });
            ui.add_space(24.0);
            for (number, text) in [
                ("1", "使用数据线连接手机"),
                ("2", "解锁手机屏幕"),
                ("3", "在手机上选择“信任”"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(number).color(d::BLUE).strong());
                    ui.label(text);
                });
                ui.add_space(8.0);
            }
            ui.add_space(18.0);
            if ui
                .add_sized([ui.available_width(), 44.0], egui::Button::new("连接帮助"))
                .clicked()
            {
                self.show_help = true;
            }
            return;
        };
        if self
            .photo
            .as_ref()
            .is_none_or(|(id, _)| id != &device.product)
            && self.photo_failed.as_deref() != Some(&device.product)
        {
            let image = self
                .runtime
                .as_ref()
                .and_then(|root| {
                    std::fs::read(root.join(apple_toolkit::models::photo_file(&device.product)))
                        .ok()
                })
                .and_then(|b| image::load_from_memory(&b).ok());
            if let Some(image) = image {
                let rgba = image.to_rgba8();
                self.photo = Some((
                    device.product.clone(),
                    ui.ctx().load_texture(
                        format!("phone-{}", device.product),
                        egui::ColorImage::from_rgba_unmultiplied(
                            [rgba.width() as usize, rgba.height() as usize],
                            rgba.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    ),
                ));
            } else {
                self.photo_failed = Some(device.product.clone());
            }
        }
        let (banner, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), 170.0),
            egui::Sense::hover(),
        );
        d::wash(
            ui.painter(),
            banner,
            Color32::from_rgb(218, 217, 233),
            Color32::from_rgb(239, 241, 249),
        );
        if let Some((id, image)) = &self.photo {
            if id == &device.product {
                let crop = if ["iPhone17,1", "iPhone17,2"].contains(&id.as_str()) {
                    egui::Rect::from_min_max(egui::pos2(0.65, 0.0), egui::pos2(1.0, 1.0))
                } else {
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0))
                };
                let size = image.size_vec2() * crop.size();
                let scale = (banner.width() * 0.78 / size.x).min(154.0 / size.y);
                ui.painter().image(
                    image.id(),
                    egui::Rect::from_center_size(banner.center(), size * scale),
                    crop,
                    Color32::WHITE,
                );
            }
        } else {
            d::icon(ui.painter(), banner.center(), 90.0, Icon::Phone, d::MUTED);
        }
        ui.add_space(14.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(apple_toolkit::models::name(&device.product))
                    .size(22.0)
                    .strong(),
            );
            ui.label(
                RichText::new("机型示意图 · 颜色仅供参考")
                    .size(12.0)
                    .color(d::MUTED),
            );
        });
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(d::SELECTED)
            .corner_radius(12)
            .inner_margin(8)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width() - 16.0);
                d::status(ui, true, "已连接 · USB");
            });
        ui.add_space(16.0);
        let fact = |key: &str| {
            device
                .facts
                .get(key)
                .cloned()
                .unwrap_or_else(|| "未获取".into())
        };
        for (label, value, copy) in [
            (
                "型号",
                apple_toolkit::models::name(&device.product).to_owned(),
                false,
            ),
            (
                "存储",
                fact("CapacityLabel").replace("（由系统容量换算）", ""),
                false,
            ),
            (
                "iOS 版本",
                format!("{} ({})", device.version, device.build),
                false,
            ),
            ("电量", fact("BatteryLevelLabel"), false),
            ("UDID", device.udid.clone(), true),
            ("序列号", device.serial.clone(), true),
            ("Wi-Fi 地址", fact("WiFiAddress"), true),
        ] {
            if info_line(ui, label, &value, copy) {
                self.notice = format!("已复制{label}");
            }
        }
        ui.add_space(14.0);
        let detail = ui.add_enabled(
            self.run.is_none(),
            egui::Button::new("查看完整设备信息").min_size(egui::vec2(ui.available_width(), 46.0)),
        );
        d::icon(
            ui.painter(),
            egui::pos2(detail.rect.right() - 18.0, detail.rect.center().y),
            17.0,
            Icon::Chevron,
            d::MUTED,
        );
        if detail.clicked() {
            self.read_details();
        }
    }
    pub(super) fn help_windows(&mut self, ctx: &egui::Context) {
        egui::Window::new("连接帮助").open(&mut self.show_help).collapsible(false).resizable(false).default_width(440.0).anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui|{
            ui.heading("连接你的 iPhone");ui.add_space(14.0);
            for text in ["1. 用支持数据传输的数据线连接电脑。","2. 解锁手机，在手机上点击“信任此电脑”。","3. 等待自动识别，也可点击“刷新设备列表”。"]{ui.label(text);ui.add_space(8.0);}
            ui.separator();ui.label("仍无法识别：尝试其他 USB 接口或数据线，并检查 Apple 移动设备驱动是否已安装。请只连接一台手机。");
        });
        let mut open_support = false;
        egui::Window::new("关于 Unbound")
            .open(&mut self.show_about)
            .collapsible(false)
            .resizable(false)
            .default_width(390.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading("Unbound");
                ui.label("0.9 · iOS 与 Android 设备管理工具");
                ui.add_space(12.0);
                ui.label("设备信息、实际读数验机、备份恢复、应用与文件管理；iOS 解锁与安卓工具。");
                ui.small("设备参数来自实际读取，未获取的项目会明确显示。");
                ui.add_space(12.0);
                if ui.button("支持这个项目").clicked() {
                    open_support = true;
                }
            });
        if open_support {
            self.show_about = false;
            self.support_open = true;
        }
    }
}
fn search_field(ui: &mut egui::Ui, value: &mut String, hint: &str, width: f32) {
    egui::Frame::new()
        .fill(Color32::from_white_alpha(210))
        .stroke(Stroke::new(1.0_f32, d::BORDER))
        .corner_radius(14)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 26.0;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                d::icon(ui.painter(), r.center(), 21.0, Icon::Search, d::MUTED);
                ui.add(
                    egui::TextEdit::singleline(value)
                        .hint_text(RichText::new(hint).color(d::MUTED))
                        .frame(false)
                        .desired_width((width - 62.0).max(100.0)),
                );
            });
        });
}
fn info_line(ui: &mut egui::Ui, label: &str, value: &str, copy: bool) -> bool {
    let mut copied = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [66.0, 23.0],
            egui::Label::new(RichText::new(label).size(14.0).color(d::MUTED)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if copy {
                let response = ui
                    .add(
                        egui::Button::new("")
                            .small()
                            .min_size(egui::vec2(26.0, 26.0))
                            .frame(false),
                    )
                    .on_hover_text(format!("复制{label}"));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("复制{label}"),
                    )
                });
                d::icon(
                    ui.painter(),
                    response.rect.center(),
                    15.0,
                    Icon::Copy,
                    d::MUTED,
                );
                if response.clicked() {
                    ui.ctx().copy_text(value.to_owned());
                    copied = true;
                }
            }
            let display = if value.chars().count() > 23 {
                format!(
                    "{}…{}",
                    value.chars().take(9).collect::<String>(),
                    value
                        .chars()
                        .rev()
                        .take(7)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                )
            } else {
                value.into()
            };
            ui.label(RichText::new(display).size(14.0))
                .on_hover_text(value);
        });
    });
    copied
}
