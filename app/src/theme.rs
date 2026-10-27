//! PLUME's visual language: ink, ivory and a few warm accents.
//!
//! Everything here is original artwork generated from code. No external fonts,
//! icons or image assets are loaded, which keeps the desktop harness portable
//! and makes the same language easy to reproduce in the Android shell.

use egui::{
    epaint::PathStroke, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, Pos2,
    Rect, Stroke, StrokeKind, TextStyle, Vec2, Visuals,
};
use plume_core::brush::{Brush, ProfileShape};
use plume_core::pattern::PatternKind;
use plume_core::Vec4;

// Ink / paper family.
pub const INK: Color32 = Color32::from_rgb(18, 20, 23);
pub const INK_SOFT: Color32 = Color32::from_rgb(25, 28, 32);
pub const PANEL: Color32 = Color32::from_rgb(31, 34, 39);
pub const PANEL_ALT: Color32 = Color32::from_rgb(39, 43, 49);
pub const PANEL_HIGH: Color32 = Color32::from_rgb(48, 53, 60);
pub const LINE: Color32 = Color32::from_rgb(63, 69, 77);
pub const LINE_SOFT: Color32 = Color32::from_rgb(48, 53, 59);
pub const IVORY: Color32 = Color32::from_rgb(240, 234, 220);
pub const MUTED: Color32 = Color32::from_rgb(155, 154, 146);
pub const FAINT: Color32 = Color32::from_rgb(108, 109, 108);

// Accents.
pub const CORAL: Color32 = Color32::from_rgb(236, 110, 81);
pub const CORAL_DEEP: Color32 = Color32::from_rgb(188, 76, 56);
pub const TEAL: Color32 = Color32::from_rgb(99, 197, 181);
pub const GOLD: Color32 = Color32::from_rgb(224, 183, 97);
pub const LILAC: Color32 = Color32::from_rgb(167, 151, 236);
pub const RED: Color32 = Color32::from_rgb(224, 94, 94);
pub const GREEN: Color32 = Color32::from_rgb(111, 190, 128);
pub const BLUE: Color32 = Color32::from_rgb(108, 160, 230);

// The viewport surround is darker than the panels so the artwork carries the
// frame.
pub const VOID: Color32 = Color32::from_rgb(13, 15, 18);

/// Install the PLUME style on an egui context.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();

    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = INK;
    visuals.faint_bg_color = PANEL_ALT;
    visuals.code_bg_color = INK_SOFT;
    visuals.override_text_color = Some(IVORY);
    visuals.weak_text_color = Some(MUTED);
    visuals.weak_text_alpha = 0.9;
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.selection.bg_fill = CORAL.gamma_multiply(0.38);
    visuals.selection.stroke = Stroke::new(1.0, CORAL);
    visuals.hyperlink_color = TEAL;
    visuals.warn_fg_color = GOLD;
    visuals.error_fg_color = RED;
    visuals.button_frame = true;
    visuals.collapsing_header_frame = false;
    visuals.indent_has_left_vline = false;
    visuals.striped = true;
    visuals.slider_trailing_fill = true;
    visuals.resize_corner_size = 10.0;

    let corner = CornerRadius::same(6);
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE_SOFT);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, IVORY);
    visuals.widgets.noninteractive.corner_radius = corner;

    visuals.widgets.inactive.bg_fill = PANEL_ALT;
    visuals.widgets.inactive.weak_bg_fill = PANEL_ALT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE_SOFT);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, IVORY.gamma_multiply(0.92));
    visuals.widgets.inactive.corner_radius = corner;
    visuals.widgets.inactive.expansion = 0.0;

    visuals.widgets.hovered.bg_fill = PANEL_HIGH;
    visuals.widgets.hovered.weak_bg_fill = PANEL_HIGH;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, CORAL.gamma_multiply(0.8));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.4, IVORY);
    visuals.widgets.hovered.corner_radius = corner;
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = CORAL_DEEP;
    visuals.widgets.active.weak_bg_fill = CORAL_DEEP;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, CORAL);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, IVORY);
    visuals.widgets.active.corner_radius = corner;
    visuals.widgets.active.expansion = 0.0;

    visuals.widgets.open.bg_fill = PANEL_ALT;
    visuals.widgets.open.weak_bg_fill = PANEL_ALT;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, TEAL.gamma_multiply(0.7));
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, IVORY);
    visuals.widgets.open.corner_radius = corner;

    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.menu_margin = Margin::same(8);
    style.spacing.indent = 16.0;
    style.spacing.slider_width = 130.0;
    style.spacing.combo_width = 140.0;
    style.spacing.interact_size = Vec2::new(28.0, 22.0);
    style.spacing.icon_width = 16.0;
    style.spacing.icon_width_inner = 10.0;
    style.animation_time = 0.12;
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(10.5, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Heading,
            FontId::new(18.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn panel_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(12, 10))
        .stroke(Stroke::new(1.0, LINE_SOFT))
}

pub fn bar_frame() -> Frame {
    Frame::new()
        .fill(INK_SOFT)
        .inner_margin(Margin::symmetric(14, 8))
        .stroke(Stroke::new(1.0, LINE_SOFT))
}

pub fn pill(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) {
    let text = text.into();
    Frame::new()
        .fill(color.gamma_multiply(0.16))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.7)))
        .corner_radius(CornerRadius::same(9))
        .inner_margin(Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(10.5).color(color));
        });
}

pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(1.0);
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(9.5)
                .strong()
                .color(FAINT),
        );
        ui.add_space(2.0);
        let remaining = ui.available_width();
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(remaining.max(0.0), 1.0), egui::Sense::hover());
        ui.painter()
            .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, LINE_SOFT));
    });
}

pub fn heading(ui: &mut egui::Ui, title: &str, subtitle: Option<&str>) {
    ui.label(egui::RichText::new(title).size(15.0).strong().color(IVORY));
    if let Some(subtitle) = subtitle {
        ui.label(egui::RichText::new(subtitle).size(11.0).color(MUTED));
    }
}

pub fn divider(ui: &mut egui::Ui) {
    ui.add_space(2.0);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, LINE_SOFT));
    ui.add_space(2.0);
}

pub fn color32(v: Vec4) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (v.x.clamp(0.0, 1.0) * 255.0).round() as u8,
        (v.y.clamp(0.0, 1.0) * 255.0).round() as u8,
        (v.z.clamp(0.0, 1.0) * 255.0).round() as u8,
        (v.w.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

pub fn vec4(c: Color32, alpha: f32) -> Vec4 {
    Vec4::new(
        c.r() as f32 / 255.0,
        c.g() as f32 / 255.0,
        c.b() as f32 / 255.0,
        alpha.clamp(0.0, 1.0),
    )
}

/// A compact colour button with a label. Returns true when the colour changed.
pub fn color_edit(ui: &mut egui::Ui, label: &str, color: &mut Vec4) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(11.5).color(MUTED));
        ui.add_space(2.0);
        let mut c = color32(*color);
        let response = ui.color_edit_button_srgba(&mut c);
        if response.changed() {
            *color = vec4(c, color.w);
            changed = true;
        }
        // Keep the hex readout inside narrow inspectors instead of pushing it
        // through the panel edge. The swatch alone is enough when space is tight.
        if ui.available_width() > 52.0 {
            ui.add_space(ui.available_width() - 46.0);
            ui.label(
                egui::RichText::new(format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b()))
                    .monospace()
                    .size(9.5)
                    .color(FAINT),
            );
        }
    });
    changed
}

/// Original PLUME wordmark: a quill arc and spaced capitals.
pub fn wordmark(ui: &mut egui::Ui) {
    let width = 132.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 34.0), egui::Sense::hover());
    let painter = ui.painter();
    // Three feather strokes suggest a quill without borrowing an icon set.
    let base = Pos2::new(rect.left() + 8.0, rect.bottom() - 6.0);
    for (i, dy) in [0.0_f32, 5.0, 10.0].iter().enumerate() {
        let lift = 17.0 - i as f32 * 3.0;
        let pts = vec![
            base + Vec2::new(0.0, -dy * 0.35),
            base + Vec2::new(3.0, -lift * 0.45 - dy * 0.1),
            base + Vec2::new(10.0, -lift - dy * 0.2),
        ];
        painter.add(egui::Shape::line(
            pts,
            PathStroke::new(
                if i == 0 { 1.4 } else { 0.9 },
                CORAL.gamma_multiply(0.9 - i as f32 * 0.22),
            ),
        ));
    }
    painter.circle_filled(base, 1.5, CORAL);
    painter.text(
        Pos2::new(rect.left() + 24.0, rect.center().y - 1.0),
        Align2::LEFT_CENTER,
        "P L U M E",
        FontId::new(17.0, FontFamily::Proportional),
        IVORY,
    );
}

/// Painted tool icons. They are deliberately simple strokes so they stay
/// legible on the dark panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Brush,
    Eraser,
    Liquify,
    Select,
    Move,
    Guide,
    Line,
    Circle,
    Ellipse,
    Smooth,
    Play,
    Pause,
    Stop,
    Camera,
    Plus,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Trash,
    Undo,
    Redo,
    Sparkle,
    Grid,
    Close,
    Check,
    Rewind,
    Key,
}

pub fn icon(painter: &egui::Painter, rect: Rect, kind: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.32;
    let s = Stroke::new(1.4, color);
    let thin = Stroke::new(1.0, color.gamma_multiply(0.75));
    let dot = |p: Pos2| painter.circle_filled(p, 1.4, color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], s);
    match kind {
        Icon::Brush => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-r * 0.9, r * 0.9),
                    c + Vec2::new(-r * 0.1, r * 0.1),
                    c + Vec2::new(r * 0.7, -r * 0.75),
                ],
                PathStroke::new(2.0, color),
            ));
            painter.circle_stroke(c + Vec2::new(-r * 0.1, r * 0.1), r * 0.65, thin);
            dot(c + Vec2::new(-r * 0.9, r * 0.9));
        }
        Icon::Eraser => {
            let block =
                Rect::from_center_size(c + Vec2::new(-r * 0.15, 0.0), Vec2::new(r * 1.4, r * 0.85));
            painter.rect(
                block,
                CornerRadius::same(2),
                Color32::TRANSPARENT,
                s,
                StrokeKind::Middle,
            );
            line(
                c + Vec2::new(-r * 0.75, r * 0.75),
                c + Vec2::new(r * 0.6, r * 0.75),
            );
            line(
                c + Vec2::new(r * 0.05, -r * 0.4),
                c + Vec2::new(r * 0.6, r * 0.4),
            );
        }
        Icon::Liquify => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-r, r * 0.4),
                    c + Vec2::new(-r * 0.4, -r * 0.2),
                    c + Vec2::new(0.0, r * 0.1),
                    c + Vec2::new(r * 0.5, -r * 0.8),
                ],
                PathStroke::new(1.5, color),
            ));
            painter.circle_stroke(c + Vec2::new(r * 0.6, r * 0.5), r * 0.28, thin);
        }
        Icon::Select => {
            let pts = vec![
                c + Vec2::new(-r * 0.7, -r),
                c + Vec2::new(-r * 0.7, r * 0.55),
                c + Vec2::new(-r * 0.15, r * 0.15),
                c + Vec2::new(r * 0.2, r * 0.95),
                c + Vec2::new(r * 0.5, r * 0.78),
                c + Vec2::new(r * 0.15, 0.0),
                c + Vec2::new(r * 0.8, -r * 0.05),
            ];
            painter.add(egui::Shape::closed_line(pts, s));
        }
        Icon::Move => {
            line(c + Vec2::new(-r, 0.0), c + Vec2::new(r, 0.0));
            line(c + Vec2::new(0.0, -r), c + Vec2::new(0.0, r));
            for d in [Vec2::X, -Vec2::X, Vec2::Y, -Vec2::Y] {
                let p = c + d * r;
                let n = Vec2::new(-d.y, d.x) * 3.0;
                painter.add(egui::Shape::convex_polygon(
                    vec![p, p - d * 5.0 + n, p - d * 5.0 - n],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::Guide => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(r * 1.8)),
                CornerRadius::same(1),
                thin,
                StrokeKind::Middle,
            );
            line(c + Vec2::new(-r, -r * 0.2), c + Vec2::new(r, -r * 0.2));
            line(c + Vec2::new(-r * 0.2, -r), c + Vec2::new(-r * 0.2, r));
            dot(c + Vec2::new(-r * 0.2, -r * 0.2));
        }
        Icon::Line => {
            line(
                c + Vec2::new(-r * 0.9, r * 0.8),
                c + Vec2::new(r * 0.9, -r * 0.8),
            );
        }
        Icon::Circle => {
            painter.circle_stroke(c, r, s);
        }
        Icon::Ellipse => {
            painter.add(egui::Shape::ellipse_stroke(c, Vec2::new(r, r * 0.62), s));
        }
        Icon::Smooth => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-r, r * 0.5),
                    c + Vec2::new(-r * 0.4, -r * 0.7),
                    c + Vec2::new(r * 0.2, r * 0.7),
                    c + Vec2::new(r, -r * 0.5),
                ],
                PathStroke::new(1.5, color),
            ));
        }
        Icon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(-r * 0.55, -r),
                    c + Vec2::new(-r * 0.55, r),
                    c + Vec2::new(r * 0.9, 0.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Pause => {
            let w = r * 0.45;
            painter.rect_filled(
                Rect::from_center_size(c + Vec2::new(-w, 0.0), Vec2::new(w * 0.9, r * 1.8)),
                CornerRadius::same(1),
                color,
            );
            painter.rect_filled(
                Rect::from_center_size(c + Vec2::new(w, 0.0), Vec2::new(w * 0.9, r * 1.8)),
                CornerRadius::same(1),
                color,
            );
        }
        Icon::Stop => {
            painter.rect_filled(
                Rect::from_center_size(c, Vec2::splat(r * 1.3)),
                CornerRadius::same(2),
                color,
            );
        }
        Icon::Camera => {
            painter.rect_stroke(
                Rect::from_center_size(c, Vec2::new(r * 2.0, r * 1.4)),
                CornerRadius::same(3),
                s,
                StrokeKind::Middle,
            );
            painter.circle_stroke(c, r * 0.48, thin);
            painter.rect_filled(
                Rect::from_center_size(
                    c + Vec2::new(-r * 0.55, -r * 0.82),
                    Vec2::new(r * 0.7, r * 0.24),
                ),
                CornerRadius::same(1),
                color,
            );
        }
        Icon::Plus => {
            line(c - Vec2::X * r, c + Vec2::X * r);
            line(c - Vec2::Y * r, c + Vec2::Y * r);
        }
        Icon::Eye | Icon::EyeOff => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-r, 0.0),
                    c + Vec2::new(-r * 0.45, -r * 0.55),
                    c + Vec2::new(0.0, -r * 0.68),
                    c + Vec2::new(r * 0.45, -r * 0.55),
                    c + Vec2::new(r, 0.0),
                    c + Vec2::new(r * 0.45, r * 0.55),
                    c + Vec2::new(0.0, r * 0.68),
                    c + Vec2::new(-r * 0.45, r * 0.55),
                ],
                PathStroke::new(1.1, color),
            ));
            if kind == Icon::Eye {
                painter.circle_filled(c, r * 0.28, color);
            } else {
                line(c + Vec2::new(-r, r), c + Vec2::new(r, -r));
            }
        }
        Icon::Lock | Icon::Unlock => {
            painter.rect_stroke(
                Rect::from_center_size(c + Vec2::new(0.0, r * 0.28), Vec2::new(r * 1.3, r)),
                CornerRadius::same(2),
                thin,
                StrokeKind::Middle,
            );
            let arc = if kind == Icon::Lock {
                vec![
                    c + Vec2::new(-r * 0.48, 0.0),
                    c + Vec2::new(-r * 0.48, -r * 0.55),
                    c + Vec2::new(0.0, -r * 0.92),
                    c + Vec2::new(r * 0.48, -r * 0.55),
                    c + Vec2::new(r * 0.48, 0.0),
                ]
            } else {
                vec![
                    c + Vec2::new(-r * 0.48, 0.0),
                    c + Vec2::new(-r * 0.48, -r * 0.55),
                    c + Vec2::new(0.0, -r * 0.92),
                    c + Vec2::new(r * 0.55, -r * 0.72),
                ]
            };
            painter.add(egui::Shape::line(arc, s));
        }
        Icon::Trash => {
            painter.rect_stroke(
                Rect::from_center_size(c + Vec2::new(0.0, r * 0.2), Vec2::new(r * 1.2, r * 1.35)),
                CornerRadius::same(1),
                thin,
                StrokeKind::Middle,
            );
            line(
                c + Vec2::new(-r * 0.8, -r * 0.55),
                c + Vec2::new(r * 0.8, -r * 0.55),
            );
            line(
                c + Vec2::new(-r * 0.28, -r * 0.9),
                c + Vec2::new(r * 0.28, -r * 0.9),
            );
        }
        Icon::Undo | Icon::Redo => {
            let dir = if kind == Icon::Undo { -1.0 } else { 1.0 };
            let pts = vec![
                c + Vec2::new(dir * r * 0.75, r * 0.5),
                c + Vec2::new(dir * r * 0.2, -r * 0.2),
                c + Vec2::new(dir * -r * 0.55, -r * 0.25),
            ];
            painter.add(egui::Shape::line(pts, PathStroke::new(1.4, color)));
            let p = c + Vec2::new(dir * r * 0.75, r * 0.5);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    p,
                    p + Vec2::new(-dir * 5.0, -2.0),
                    p + Vec2::new(-dir * 2.0, 5.0),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Sparkle => {
            for (angle, len) in [(0.0_f32, 1.0_f32), (0.7, 0.55), (-0.7, 0.55), (1.45, 0.4)] {
                let d = Vec2::angled(angle);
                line(c - d * r * len, c + d * r * len);
            }
            painter.circle_filled(c, 1.3, color);
        }
        Icon::Grid => {
            for i in -1..=1 {
                let x = c.x + i as f32 * r * 0.65;
                let y = c.y + i as f32 * r * 0.65;
                painter.line_segment([Pos2::new(x, c.y - r), Pos2::new(x, c.y + r)], thin);
                painter.line_segment([Pos2::new(c.x - r, y), Pos2::new(c.x + r, y)], thin);
            }
        }
        Icon::Close => {
            line(c - Vec2::splat(r * 0.7), c + Vec2::splat(r * 0.7));
            line(
                c + Vec2::new(-r * 0.7, r * 0.7),
                c + Vec2::new(r * 0.7, -r * 0.7),
            );
        }
        Icon::Check => {
            painter.add(egui::Shape::line(
                vec![
                    c + Vec2::new(-r * 0.8, 0.0),
                    c + Vec2::new(-r * 0.2, r * 0.6),
                    c + Vec2::new(r * 0.85, -r * 0.6),
                ],
                PathStroke::new(1.8, color),
            ));
        }
        Icon::Rewind => {
            for dx in [-0.55_f32, 0.45] {
                let base = c + Vec2::new(dx * r, 0.0);
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        base + Vec2::new(r * 0.45, -r * 0.75),
                        base + Vec2::new(r * 0.45, r * 0.75),
                        base + Vec2::new(-r * 0.45, 0.0),
                    ],
                    color,
                    Stroke::NONE,
                ));
            }
        }
        Icon::Key => {
            painter.circle_stroke(c + Vec2::new(-r * 0.5, -r * 0.35), r * 0.4, s);
            line(
                c + Vec2::new(-r * 0.2, -r * 0.05),
                c + Vec2::new(r * 0.9, r * 0.9),
            );
            line(
                c + Vec2::new(r * 0.3, r * 0.3),
                c + Vec2::new(r * 0.05, r * 0.55),
            );
        }
    }
}

/// A tool button with a custom painted icon and a short caption.
pub fn tool_button(
    ui: &mut egui::Ui,
    selected: bool,
    icon_kind: Icon,
    label: &str,
    hint: &str,
) -> bool {
    let size = Vec2::new(ui.available_width(), 40.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let hovered = response.hovered();
    let active = selected || hovered;
    let bg = if selected {
        CORAL.gamma_multiply(0.22)
    } else if hovered {
        PANEL_HIGH
    } else {
        PANEL_ALT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        bg,
        Stroke::new(
            1.0,
            if selected {
                CORAL.gamma_multiply(0.85)
            } else {
                LINE_SOFT
            },
        ),
        StrokeKind::Inside,
    );
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 21.0, rect.center().y),
        Vec2::splat(24.0),
    );
    icon(
        ui.painter(),
        icon_rect,
        icon_kind,
        if active { IVORY } else { MUTED },
    );
    ui.painter().text(
        Pos2::new(rect.left() + 39.0, rect.center().y - 1.0),
        Align2::LEFT_CENTER,
        label,
        FontId::new(12.0, FontFamily::Proportional),
        if active {
            IVORY
        } else {
            IVORY.gamma_multiply(0.82)
        },
    );
    response.on_hover_text(hint).clicked()
}

/// Paint a procedural preview of a brush stroke.
pub fn brush_preview(painter: &egui::Painter, rect: Rect, brush: &Brush, dim: bool) {
    let painter = painter.with_clip_rect(rect);
    let inset = rect.shrink2(Vec2::new(10.0, 7.0));
    let color = color32(brush.color).gamma_multiply(if dim { 0.55 } else { 1.0 });
    let n = 30;
    let mut pts = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let x = inset.left() + inset.width() * t;
        let envelope = (t * std::f32::consts::PI).sin().powf(0.65);
        let y = inset.center().y + (t * 8.0).sin() * inset.height() * 0.26 * envelope;
        pts.push(Pos2::new(x, y));
    }
    let max_r = (inset.height() * 0.30).clamp(2.0, 10.0);
    let scale = (max_r / brush.base_size.max(0.5)).clamp(0.28, 2.2);
    let pattern_alpha = match brush.pattern.kind {
        PatternKind::None => 1.0,
        _ => 0.85,
    };
    for (i, pair) in pts.windows(2).enumerate() {
        let u = i as f32 / (n - 1) as f32;
        let pressure = 0.55 + 0.45 * (u * std::f32::consts::PI).sin();
        let radius = (brush.preview_radius(u, pressure) * scale).clamp(0.6, inset.height() * 0.48);
        let width = radius
            * if brush.profile.shape == ProfileShape::Flat {
                1.4
            } else {
                2.0
            };
        let alpha = (brush.opacity * pattern_alpha * if dim { 0.7 } else { 1.0 }).clamp(0.15, 1.0);
        painter.line_segment(
            [pair[0], pair[1]],
            Stroke::new(width, color.gamma_multiply(alpha)),
        );
    }
    // Pattern overlay uses the same categories as the renderer.
    let pattern_color = color32(brush.pattern.color_b).gamma_multiply(0.8);
    match brush.pattern.kind {
        PatternKind::None => {}
        PatternKind::Halftone => {
            let spacing = (brush.pattern.spacing.max(0.05) * 44.0).clamp(8.0, 18.0);
            let radius = (brush.pattern.thickness.max(0.05) * spacing * 0.7).clamp(1.0, 3.2);
            let mut y = inset.top() + 6.0;
            let mut row = 0;
            while y < inset.bottom() {
                let mut x = inset.left() + if row % 2 == 0 { 0.0 } else { spacing * 0.5 };
                while x < inset.right() {
                    painter.circle_filled(Pos2::new(x, y), radius, color.gamma_multiply(0.55));
                    x += spacing;
                }
                row += 1;
                y += spacing;
            }
        }
        PatternKind::Hatch | PatternKind::CrossHatch => {
            let spacing = 7.0;
            let mut x = inset.left() - inset.height();
            while x < inset.right() {
                painter.line_segment(
                    [
                        Pos2::new(x, inset.bottom()),
                        Pos2::new(x + inset.height(), inset.top()),
                    ],
                    Stroke::new(1.0, pattern_color.gamma_multiply(0.65)),
                );
                if brush.pattern.kind == PatternKind::CrossHatch {
                    painter.line_segment(
                        [
                            Pos2::new(x, inset.top()),
                            Pos2::new(x + inset.height(), inset.bottom()),
                        ],
                        Stroke::new(0.8, pattern_color.gamma_multiply(0.35)),
                    );
                }
                x += spacing;
            }
        }
        PatternKind::Stipple | PatternKind::Terrazzo => {
            let count = if brush.pattern.kind == PatternKind::Terrazzo {
                26
            } else {
                40
            };
            for i in 0..count {
                let fx = ((i * 37 + 11) % 101) as f32 / 100.0;
                let fy = ((i * 61 + 7) % 97) as f32 / 96.0;
                let p = inset.lerp_inside(Vec2::new(fx, fy));
                let radius = if brush.pattern.kind == PatternKind::Terrazzo {
                    1.0 + ((i * 13) % 5) as f32 * 0.45
                } else {
                    0.7
                };
                painter.circle_filled(p, radius, pattern_color.gamma_multiply(0.7));
            }
        }
        PatternKind::Custom => {
            painter.rect_stroke(
                inset,
                CornerRadius::same(2),
                Stroke::new(1.0, GOLD.gamma_multiply(0.6)),
                StrokeKind::Inside,
            );
        }
    }
}

pub fn brush_chip(ui: &mut egui::Ui, brush: &Brush, selected: bool, show_name: bool) -> bool {
    let height = if show_name { 46.0 } else { 38.0 };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::click(),
    );
    let bg = if selected {
        CORAL.gamma_multiply(0.18)
    } else if response.hovered() {
        PANEL_HIGH
    } else {
        PANEL_ALT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        bg,
        Stroke::new(1.0, if selected { CORAL } else { LINE_SOFT }),
        StrokeKind::Inside,
    );
    let preview_rect = Rect::from_min_max(
        rect.min + Vec2::new(5.0, 4.0),
        Pos2::new(
            rect.max.x - if show_name { 118.0 } else { 30.0 },
            rect.max.y - 4.0,
        ),
    );
    brush_preview(
        ui.painter(),
        preview_rect,
        brush,
        !selected && !response.hovered(),
    );
    if show_name {
        ui.painter().text(
            Pos2::new(rect.max.x - 108.0, rect.center().y),
            Align2::LEFT_CENTER,
            &brush.name,
            FontId::new(11.0, FontFamily::Proportional),
            IVORY.gamma_multiply(if selected { 1.0 } else { 0.78 }),
        );
    }
    if selected {
        icon(
            ui.painter(),
            Rect::from_center_size(
                Pos2::new(rect.max.x - 16.0, rect.center().y),
                Vec2::splat(18.0),
            ),
            Icon::Check,
            CORAL,
        );
    }
    response.clicked()
}

/// Tiny star rating-like dots used for brush categories.
pub fn category_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 2.4, color);
}

/// Draw a translucent checker/ink plate behind thumbnails.
pub fn thumbnail_plate(painter: &egui::Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(6), INK);
    let step = 10.0;
    let mut x = rect.left();
    let mut col = 0;
    while x < rect.right() {
        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            if (row + col) % 2 == 0 {
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(x, y),
                        Pos2::new((x + step).min(rect.right()), (y + step).min(rect.bottom())),
                    ),
                    0,
                    PANEL_ALT.gamma_multiply(0.45),
                );
            }
            y += step;
            row += 1;
        }
        x += step;
        col += 1;
    }
}
