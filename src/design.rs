//! Native egui design tokens and vector icons. No web view or remote assets.
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Stroke, Vec2};
pub const BG: Color32 = Color32::from_rgb(242, 244, 248);
pub const TEXT: Color32 = Color32::from_rgb(28, 34, 46);
pub const MUTED: Color32 = Color32::from_rgb(96, 105, 122);
pub const BLUE: Color32 = Color32::from_rgb(40, 99, 228);
pub const SELECTED: Color32 = Color32::from_rgb(230, 238, 255);
pub const BORDER: Color32 = Color32::from_rgb(220, 226, 236);
pub const GREEN: Color32 = Color32::from_rgb(47, 176, 98);

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Light);
    let mut v = egui::Visuals::light();
    v.override_text_color = Some(TEXT);

    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(252, 253, 255);
    v.extreme_bg_color = Color32::from_rgb(248, 249, 252);
    v.faint_bg_color = Color32::from_rgb(246, 248, 251);
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0_f32, BLUE);
    v.window_corner_radius = 20.into();
    v.window_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.bg_fill = Color32::WHITE;
    v.widgets.inactive.weak_bg_fill = Color32::WHITE;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.inactive.corner_radius = 12.into();
    v.widgets.inactive.fg_stroke = Stroke::new(1.5_f32, TEXT);
    v.widgets.hovered = v.widgets.inactive;
    v.widgets.hovered.bg_fill = Color32::from_rgb(237, 243, 255);
    v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(156, 185, 246));
    v.widgets.active = v.widgets.hovered;
    v.widgets.active.bg_fill = SELECTED;
    v.widgets.active.weak_bg_fill = SELECTED;
    v.widgets.active.bg_stroke = Stroke::new(1.5_f32, BLUE);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(250, 252, 255);
    v.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(250, 252, 255);
    ctx.set_visuals(v);
    let mut s = (*ctx.style()).clone();
    for (kind, size) in [
        (egui::TextStyle::Heading, 26.0),
        (egui::TextStyle::Body, 16.0),
        (egui::TextStyle::Button, 15.0),
        (egui::TextStyle::Small, 13.0),
    ] {
        s.text_styles.insert(kind, FontId::proportional(size));
    }
    s.spacing.item_spacing = egui::vec2(10.0, 10.0);
    s.spacing.button_padding = egui::vec2(14.0, 10.0);
    s.spacing.interact_size = egui::vec2(44.0, 40.0);
    s.animation_time = 0.12;
    ctx.set_style(s);
}
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_white_alpha(218))
        .stroke(Stroke::new(1.0_f32, Color32::WHITE))
        .corner_radius(22)
        .inner_margin(20)
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: Color32::from_rgba_unmultiplied(51, 63, 87, 19),
        })
}
pub fn wash(p: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(mesh));
}
const BRAND_PNG: &[u8] = include_bytes!("../assets/brand/unbound.png");
fn brand_rgba() -> image::RgbaImage {
    image::load_from_memory(BRAND_PNG)
        .expect("valid embedded Unbound icon")
        .to_rgba8()
}
pub fn load_logo(ctx: &egui::Context) -> egui::TextureHandle {
    let image = brand_rgba();
    ctx.load_texture(
        "unbound-brand",
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}
pub fn window_icon() -> egui::IconData {
    let image = brand_rgba();
    egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    }
}

// ---------------------------------------------------------------- 支持二维码
//
// 两张收款码编译进程序，运行时不读磁盘。解码失败就返回 `None`，弹窗不显示 ——
// 缺一张码不至于让整个界面起不来。

pub const SUPPORT_REWARD: &[u8] = include_bytes!("../assets/support/reward.jpg");
pub const SUPPORT_ALIPAY: &[u8] = include_bytes!("../assets/support/alipay.jpg");

fn decode(bytes: &[u8]) -> Option<egui::ColorImage> {
    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [image.width() as usize, image.height() as usize],
        image.as_raw(),
    ))
}

/// 加载两张收款码纹理。任一解码失败返回 `None`。
pub fn load_support(ctx: &egui::Context) -> Option<(egui::TextureHandle, egui::TextureHandle)> {
    let reward = decode(SUPPORT_REWARD)?;
    let alipay = decode(SUPPORT_ALIPAY)?;
    Some((
        ctx.load_texture("support-reward", reward, egui::TextureOptions::LINEAR),
        ctx.load_texture("support-alipay", alipay, egui::TextureOptions::LINEAR),
    ))
}
pub fn logo(p: &egui::Painter, rect: Rect, texture: &egui::TextureHandle) {
    p.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
#[derive(Clone, Copy)]
pub enum Icon {
    Home,
    Phone,
    Info,
    Backup,
    Folder,
    Refresh,
    Search,
    Trash,
    Restart,
    More,
    Chevron,
    Check,
    Copy,
    Usb,
    Help,
}
pub fn icon(p: &egui::Painter, center: Pos2, size: f32, kind: Icon, color: Color32) {
    let pt = |x: f32, y: f32| center + egui::vec2(x - 12.0, y - 12.0) * (size / 24.0);
    let stroke = Stroke::new(1.6_f32, color);
    let line = |coords: &[(f32, f32)]| {
        p.line(coords.iter().map(|&(x, y)| pt(x, y)).collect(), stroke);
    };
    match kind {
        Icon::Home => {
            line(&[(3., 10.), (12., 3.), (21., 10.)]);
            line(&[
                (5., 9.),
                (5., 21.),
                (10., 21.),
                (10., 14.),
                (14., 14.),
                (14., 21.),
                (19., 21.),
                (19., 9.),
            ]);
        }
        Icon::Phone => {
            p.rect_stroke(
                Rect::from_min_max(pt(6., 2.), pt(18., 22.)),
                3,
                color_stroke(color),
                egui::StrokeKind::Inside,
            );
            line(&[(10., 5.), (14., 5.)]);
            line(&[(11., 19.), (13., 19.)]);
        }
        Icon::Info | Icon::Help => {
            p.circle_stroke(center, size * 0.41, stroke);
            if matches!(kind, Icon::Info) {
                p.circle_filled(pt(12., 7.), size * 0.04, color);
                line(&[(12., 11.), (12., 17.)]);
            } else {
                line(&[
                    (8., 9.),
                    (9., 6.),
                    (14., 6.),
                    (16., 9.),
                    (12., 12.),
                    (12., 14.),
                ]);
                p.circle_filled(pt(12., 18.), size * 0.04, color);
            }
        }
        Icon::Backup => {
            line(&[
                (5., 17.),
                (3., 17.),
                (2., 13.),
                (4., 9.),
                (8., 8.),
                (9., 4.),
                (14., 3.),
                (18., 6.),
                (18., 9.),
                (22., 11.),
                (22., 16.),
                (19., 18.),
            ]);
            line(&[(12., 21.), (12., 10.), (8., 14.)]);
            line(&[(12., 10.), (16., 14.)]);
        }
        Icon::Folder => {
            line(&[
                (3., 7.),
                (3., 20.),
                (21., 20.),
                (21., 7.),
                (12., 7.),
                (10., 4.),
                (3., 4.),
                (3., 7.),
                (21., 7.),
            ]);
        }
        Icon::Refresh | Icon::Restart => {
            let pts: Vec<_> = (0..=22)
                .map(|i| {
                    let a = 0.6 + i as f32 / 22.0 * 5.0;
                    pt(12. + 8. * a.cos(), 12. + 8. * a.sin())
                })
                .collect();
            p.line(pts, stroke);
            line(&[(20., 3.), (20., 9.), (14., 9.)]);
        }
        Icon::Search => {
            p.circle_stroke(pt(10., 10.), size * 0.28, stroke);
            line(&[(15., 15.), (21., 21.)]);
        }
        Icon::Trash => {
            line(&[(4., 6.), (20., 6.)]);
            line(&[(7., 6.), (8., 21.), (16., 21.), (17., 6.)]);
            line(&[(9., 3.), (15., 3.)]);
            line(&[(10., 10.), (10., 17.)]);
            line(&[(14., 10.), (14., 17.)]);
        }
        Icon::More => {
            for x in [5., 12., 19.] {
                p.circle_filled(pt(x, 12.), size * 0.065, color);
            }
        }
        Icon::Chevron => line(&[(9., 5.), (16., 12.), (9., 19.)]),
        Icon::Check => line(&[(5., 12.), (10., 17.), (20., 6.)]),
        Icon::Copy => {
            p.rect_stroke(
                Rect::from_min_max(pt(8., 7.), pt(20., 21.)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            line(&[(15., 4.), (4., 4.), (4., 16.)]);
        }
        Icon::Usb => {
            line(&[(12., 21.), (12., 3.), (9., 6.)]);
            line(&[(12., 3.), (15., 6.)]);
            line(&[(12., 16.), (6., 12.), (6., 8.)]);
            line(&[(12., 12.), (18., 9.), (18., 7.)]);
            p.circle_filled(pt(6., 7.), size * 0.065, color);
            p.rect_filled(
                Rect::from_center_size(pt(18., 6.), Vec2::splat(size * 0.13)),
                0,
                color,
            );
        }
    }
}
fn color_stroke(c: Color32) -> Stroke {
    Stroke::new(1.6_f32, c)
}
pub fn nav(
    ui: &mut egui::Ui,
    kind: Icon,
    label: &str,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(format!("       {label}"))
                .size(16.0)
                .color(if selected { BLUE } else { TEXT }),
        )
        .fill(if selected {
            SELECTED
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::NONE)
        .corner_radius(13)
        .min_size(egui::vec2(ui.available_width(), 48.0)),
    );
    icon(
        ui.painter(),
        egui::pos2(response.rect.left() + 23.0, response.rect.center().y),
        21.0,
        kind,
        if selected { BLUE } else { MUTED },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub fn tile(
    ui: &mut egui::Ui,
    kind: Icon,
    label: &str,
    width: f32,
    enabled: bool,
) -> egui::Response {
    let response = ui
        .allocate_ui_with_layout(
            egui::vec2(width, 82.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.add_enabled(
                    enabled,
                    egui::Button::new(egui::RichText::new(format!("\n\n{label}")).size(14.0))
                        .min_size(egui::vec2(width, 82.0))
                        .corner_radius(16),
                )
            },
        )
        .inner;
    icon(
        ui.painter(),
        egui::pos2(response.rect.center().x, response.rect.top() + 24.0),
        25.0,
        kind,
        if enabled { TEXT } else { MUTED },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub fn status(ui: &mut egui::Ui, connected: bool, text: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 16.0), egui::Sense::hover());
        ui.painter().circle_filled(
            r.center(),
            4.0,
            if connected {
                GREEN
            } else {
                Color32::from_rgb(154, 163, 178)
            },
        );
        ui.label(egui::RichText::new(text).size(14.0));
    });
}
