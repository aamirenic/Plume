//! PLUME's editor interface.
//!
//! The layout is deliberately recognisable without imitating another product:
//! a wordmark and command strip at the top, four workshop drawers on the left,
//! a context inspector on the right, a render surface in the middle and a
//! transport strip below.  Everything is painted from egui primitives and the
//! procedural helpers in [`crate::theme`]; no external UI assets are used.

use std::time::Duration;

use eframe::egui;
use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Layout, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, UiBuilder, Vec2,
};
use plume_core::animation::{Easing, TurntableAxis};
use plume_core::brush::{Brush, BrushCategory, ProfileShape};
use plume_core::guide::GuideKind;
use plume_core::pattern::{PatternChannel, PatternKind};
use plume_core::render::{BackgroundKind, FogMode, LightKind, Tonemap};
use plume_core::scene::{MeshQuality, MirrorAxis};
use plume_core::transform::{self, SnapMode, TransformMode};
use plume_core::viewport::ViewSnap;
use plume_core::{CurveId, GuideId, Vec3};

use crate::app::{
    ConfirmAction, Dialog, ExportFormat, ImportUnit, InspectorTab, LeftTab, PlumeApp, Tool,
    WindowParent,
};
use crate::theme;

impl eframe::App for PlumeApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.window_parent = WindowParent::from_frame(frame);
        self.install_context(&ctx);
        self.poll_media();
        self.update_animation(&ctx);
        self.autosave_tick(&ctx);
        self.handle_close_request(&ctx);
        let modal = matches!(self.dialog, Dialog::Import | Dialog::Recovery);
        if self.pending_confirm.is_some() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.pending_confirm = None;
            }
        } else if !modal {
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("plume.top_bar")
            .exact_size(54.0)
            .frame(theme::bar_frame())
            .show(ui, |ui| self.top_bar(ui));

        // The status strip is added first so that the timeline sits above it.
        egui::Panel::bottom("plume.status_strip")
            .exact_size(22.0)
            .frame(
                Frame::new()
                    .fill(theme::INK_SOFT)
                    .inner_margin(egui::Margin::symmetric(12, 2))
                    .stroke(Stroke::new(1.0, theme::LINE_SOFT)),
            )
            .show(ui, |ui| self.status_strip(ui));

        if self.show_timeline {
            egui::Panel::bottom("plume.timeline")
                .exact_size(92.0)
                .frame(theme::panel_frame())
                .show(ui, |ui| self.timeline(ui));
        }

        egui::Panel::left("plume.left_drawer")
            .exact_size(232.0)
            .frame(theme::panel_frame())
            .show(ui, |ui| self.left_drawer(ui));

        egui::Panel::right("plume.inspector")
            .exact_size(308.0)
            .frame(theme::panel_frame())
            .show(ui, |ui| self.inspector(ui));

        egui::CentralPanel::no_frame().show(ui, |ui| self.viewport(ui));

        self.dialogs(&ctx);
        self.show_toast(&ctx);
        self.finish_ui_frame(&ctx);

        if self.doc.animation.playing || self.media.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else if self.toast.is_some() {
            ctx.request_repaint_after(Duration::from_millis(140));
        }
    }
}

/// Visual and structural state for one row in the layer list.
#[derive(Clone, Copy)]
struct LayerRowState {
    visible: bool,
    locked: bool,
    selected: bool,
    indented: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseRequestOutcome {
    Allow,
    Prompt,
    KeepOpen,
}

fn close_request_outcome(
    dirty: bool,
    prompt_open: bool,
    close_approved: bool,
) -> CloseRequestOutcome {
    if close_approved {
        // The user explicitly chose to discard unsaved changes. Let the close
        // through instead of reopening the same prompt on the next frame.
        CloseRequestOutcome::Allow
    } else if prompt_open {
        CloseRequestOutcome::KeepOpen
    } else if dirty {
        CloseRequestOutcome::Prompt
    } else {
        CloseRequestOutcome::Allow
    }
}

impl PlumeApp {
    // -- global behaviour ---------------------------------------------------

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        let requested = ctx.input(|i| i.viewport().close_requested());
        if !requested {
            return;
        }
        match close_request_outcome(
            self.dirty,
            self.pending_confirm.is_some(),
            self.close_approved,
        ) {
            CloseRequestOutcome::Allow => {
                // A clean exit (or an explicit discard) must not leave a
                // recovery copy that would be offered again next launch.
                self.clear_autosave();
            }
            CloseRequestOutcome::Prompt => {
                self.pending_confirm = Some(ConfirmAction::Quit);
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
            CloseRequestOutcome::KeepOpen => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
    }

    fn request_quit(&mut self) {
        if self.dirty {
            self.pending_confirm = Some(ConfirmAction::Quit);
        } else if let Some(ctx) = self.last_ctx.as_ref() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let editing = ctx.text_edit_focused();
        let mut command_shift = egui::Modifiers::COMMAND;
        command_shift.shift = true;

        // Most specific combinations first: consume_key matches logically and
        // ignores extra Shift/Alt modifiers.
        let save_as = ctx.input_mut(|i| i.consume_key(command_shift, egui::Key::S));
        let save = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        let redo_alt = ctx.input_mut(|i| i.consume_key(command_shift, egui::Key::Z));
        let redo = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y));
        let undo = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
        let open = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O));
        let new = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::N));
        let duplicate = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::D));
        let export = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::E));
        let ungroup = ctx.input_mut(|i| i.consume_key(command_shift, egui::Key::G));
        let group = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::G));

        if save_as {
            self.save_as();
        } else if save {
            self.save_current();
        }
        if redo_alt || redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if open {
            self.request_open();
        }
        if new {
            self.request_new();
        }
        if duplicate {
            self.duplicate_selection();
        }
        if export {
            self.dialog = Dialog::Export;
        }
        if ungroup {
            self.ungroup_selection();
        } else if group {
            self.group_selection();
        }

        if editing {
            return;
        }

        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            if self.dialog != Dialog::None {
                self.dialog = Dialog::None;
            } else if self.pending_confirm.is_some() {
                self.pending_confirm = None;
            } else if !matches!(self.drag, crate::app::Drag::None) {
                self.cancel_gesture();
                self.drag = crate::app::Drag::None;
            } else {
                self.doc.scene.selection.clear();
                self.needs_render = true;
            }
        }

        let delete = ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        });
        if delete {
            self.delete_selection();
        }

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.toggle_playback();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F)) {
            self.frame_selection();
        }
        let frame_artwork =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Home));
        if frame_artwork {
            self.frame_artwork();
        } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Home)) {
            self.frame_view();
        }

        for tool in Tool::ALL {
            let Some(key) = tool.shortcut() else { continue };
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                self.tool = tool;
                self.status = tool.hint().to_string();
            }
        }

        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Num1)) {
            self.snap_view(ViewSnap::Perspective);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Num2)) {
            self.snap_view(ViewSnap::Top);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Num3)) {
            self.snap_view(ViewSnap::Front);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Num4)) {
            self.snap_view(ViewSnap::Right);
        }

        let bracket_open =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket));
        let bracket_close =
            ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket));
        if bracket_open {
            self.settings.size = (self.settings.size * 0.88).max(0.2);
        }
        if bracket_close {
            self.settings.size = (self.settings.size * 1.14).min(120.0);
        }
    }

    // -- top bar ------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            theme::wordmark(ui);
            ui.add_space(4.0);

            ui.menu_button("File", |ui| {
                if ui.button("New scene          Ctrl+N").clicked() {
                    self.request_new();
                    ui.close();
                }
                if ui.button("Open…              Ctrl+O").clicked() {
                    self.request_open();
                    ui.close();
                }
                if self.has_unclaimed_autosave() && ui.button("Recover autosave…").clicked() {
                    self.request_recover_autosave();
                    ui.close();
                }
                if ui.button("Save               Ctrl+S").clicked() {
                    self.save_current();
                    ui.close();
                }
                if ui.button("Save as…       Ctrl+Shift+S").clicked() {
                    self.save_as();
                    ui.close();
                }
                ui.separator();
                if ui.button("Import mesh as guides…").clicked() {
                    self.import_mesh_dialog();
                    ui.close();
                }
                if ui.button("Scene gallery…").clicked() {
                    self.dialog = Dialog::Gallery;
                    ui.close();
                }
                if ui.button("Export…            Ctrl+E").clicked() {
                    self.dialog = Dialog::Export;
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request_quit();
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                let undo_label = self
                    .history
                    .undo_label()
                    .map(|l| format!("Undo {l}"))
                    .unwrap_or_else(|| "Undo".to_string());
                let redo_label = self
                    .history
                    .redo_label()
                    .map(|l| format!("Redo {l}"))
                    .unwrap_or_else(|| "Redo".to_string());
                if ui
                    .add_enabled(
                        self.history.can_undo(),
                        egui::Button::new(format!("{undo_label}    Ctrl+Z")),
                    )
                    .clicked()
                {
                    self.undo();
                    ui.close();
                }
                if ui
                    .add_enabled(
                        self.history.can_redo(),
                        egui::Button::new(format!("{redo_label}    Ctrl+Y")),
                    )
                    .clicked()
                {
                    self.redo();
                    ui.close();
                }
                ui.separator();
                if ui.button("Duplicate       Ctrl+D").clicked() {
                    self.duplicate_selection();
                    ui.close();
                }
                if ui.button("Delete            Del").clicked() {
                    self.delete_selection();
                    ui.close();
                }
                if ui.button("Group             Ctrl+G").clicked() {
                    self.group_selection();
                    ui.close();
                }
                if ui.button("Ungroup     Ctrl+Shift+G").clicked() {
                    self.ungroup_selection();
                    ui.close();
                }
                if ui.button("Mirror selection").clicked() {
                    self.mirror_selection();
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                if ui
                    .checkbox(&mut self.show_guides, "Construction guides")
                    .changed()
                {
                    self.needs_render = true;
                }
                ui.checkbox(&mut self.show_stats, "Viewport statistics");
                ui.checkbox(&mut self.show_timeline, "Timeline");
                ui.separator();
                if ui.button("Frame selection        F").clicked() {
                    self.frame_selection();
                    ui.close();
                }
                if ui.button("Frame scene          Home").clicked() {
                    self.frame_view();
                    ui.close();
                }
                if ui.button("Frame artwork   Shift+Home").clicked() {
                    self.frame_artwork();
                    ui.close();
                }
                ui.separator();
                for snap in [
                    ViewSnap::Perspective,
                    ViewSnap::Top,
                    ViewSnap::Bottom,
                    ViewSnap::Front,
                    ViewSnap::Rear,
                    ViewSnap::Left,
                    ViewSnap::Right,
                ] {
                    if ui.button(snap.label()).clicked() {
                        self.snap_view(snap);
                        ui.close();
                    }
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Quick start and shortcuts").clicked() {
                    self.dialog = Dialog::Help;
                    ui.close();
                }
                if ui.button("About PLUME").clicked() {
                    self.dialog = Dialog::About;
                    ui.close();
                }
            });

            ui.add_space(8.0);
            ui.label(
                RichText::new(if self.doc.name.trim().is_empty() {
                    "Untitled"
                } else {
                    &self.doc.name
                })
                .size(12.5)
                .color(theme::MUTED),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let export = ui.add(
                    egui::Button::new(RichText::new("Export").strong().color(theme::INK))
                        .fill(theme::CORAL)
                        .stroke(Stroke::NONE),
                );
                if export
                    .on_hover_text("Export a model, image or animation")
                    .clicked()
                {
                    self.dialog = Dialog::Export;
                }
                if icon_button(
                    ui,
                    theme::Icon::Grid,
                    self.dialog == Dialog::Gallery,
                    "Scene gallery",
                )
                .clicked()
                {
                    self.gallery_selected = self.gallery_selected.clone();
                    self.dialog = Dialog::Gallery;
                }
                if icon_button(ui, theme::Icon::Sparkle, false, "Render settings").clicked() {
                    self.inspector = InspectorTab::Render;
                }
                if icon_button(ui, theme::Icon::Redo, self.history.can_redo(), "Redo").clicked() {
                    self.redo();
                }
                if icon_button(ui, theme::Icon::Undo, self.history.can_undo(), "Undo").clicked() {
                    self.undo();
                }
                if self.media.is_some() {
                    ui.add(egui::Spinner::new().size(16.0));
                }
            });
        });
    }

    // -- status strip -------------------------------------------------------

    fn status_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_centered(|ui| {
            let dot = if self.dirty {
                theme::GOLD
            } else {
                theme::GREEN
            };
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 2.5, dot);
            ui.label(
                RichText::new(if self.dirty { "Unsaved" } else { "Saved" })
                    .size(10.0)
                    .color(theme::FAINT),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(&self.status).size(10.5).color(
                if self.toast.as_ref().map(|t| t.error).unwrap_or(false) {
                    theme::RED
                } else {
                    theme::MUTED
                },
            ));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                if let Some(job) = &self.media {
                    ui.label(
                        RichText::new(format!("{}  {:.0}%", job.detail, job.progress * 100.0))
                            .size(10.0)
                            .color(theme::TEAL),
                    );
                    ui.add(
                        egui::ProgressBar::new(job.progress)
                            .desired_width(150.0)
                            .desired_height(7.0)
                            .fill(theme::TEAL),
                    );
                } else {
                    let stats = self.doc.stats();
                    ui.label(
                        RichText::new(format!(
                            "{} curves · {} guides · {} points",
                            stats.curves, stats.guides, stats.control_points
                        ))
                        .size(10.0)
                        .color(theme::FAINT),
                    );
                    ui.label(
                        RichText::new(self.quality.label())
                            .size(10.0)
                            .color(theme::FAINT),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} × {} px",
                            self.last_render_size[0], self.last_render_size[1]
                        ))
                        .size(10.0)
                        .color(theme::FAINT),
                    );
                }
            });
        });
    }

    // -- left drawer --------------------------------------------------------

    fn left_drawer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for tab in LeftTab::ALL {
                let selected = self.left_tab == tab;
                let response = ui.add(
                    egui::Button::new(RichText::new(tab.label()).size(10.5).color(if selected {
                        theme::IVORY
                    } else {
                        theme::MUTED
                    }))
                    .fill(if selected {
                        theme::CORAL.gamma_multiply(0.22)
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(if selected {
                        Stroke::new(1.0, theme::CORAL.gamma_multiply(0.75))
                    } else {
                        Stroke::NONE
                    })
                    .corner_radius(CornerRadius::same(5))
                    .min_size(Vec2::new(51.0, 24.0)),
                );
                if response.clicked() {
                    self.left_tab = tab;
                }
            }
        });
        theme::divider(ui);

        egui::ScrollArea::vertical()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.left_tab {
                LeftTab::Tools => self.tools_panel(ui),
                LeftTab::Brushes => self.brushes_panel(ui),
                LeftTab::Guides => self.guides_panel(ui),
                LeftTab::Layers => self.layers_panel(ui),
            });
    }

    fn tools_panel(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "Tools");
        for tool in Tool::ALL {
            if theme::tool_button(
                ui,
                self.tool == tool,
                tool.icon(),
                tool.label(),
                tool.hint(),
            ) {
                self.tool = tool;
                self.status = tool.hint().to_string();
            }
        }

        theme::divider(ui);
        theme::section_label(ui, "Tool options");
        match self.tool {
            Tool::Brush => {
                ui.add(
                    egui::Slider::new(&mut self.settings.size, 0.2..=80.0)
                        .text("Size (mm)")
                        .logarithmic(true),
                );
                ui.add(egui::Slider::new(&mut self.settings.pressure, 0.05..=1.0).text("Pressure"));
                ui.add(
                    egui::Slider::new(&mut self.settings.smoothing, 0.0..=0.95).text("Smoothing"),
                );
                ui.add(egui::Slider::new(&mut self.settings.spacing, 0.2..=8.0).text("Spacing"));
                ui.checkbox(&mut self.settings.stable_stroke, "Stabilise stroke");
                ui.checkbox(&mut self.settings.draw_on_guide, "Draw on guide surfaces");
                snap_combo(ui, "Snap", &mut self.settings.snap);
            }
            Tool::Erase => {
                ui.label(
                    RichText::new(
                        "Drag across strokes to remove them. The tolerance follows the brush size.",
                    )
                    .size(11.0)
                    .color(theme::MUTED),
                );
                ui.add(
                    egui::Slider::new(&mut self.settings.size, 0.2..=80.0)
                        .text("Target size (mm)")
                        .logarithmic(true),
                );
            }
            Tool::Liquify => {
                let brush = &mut self.settings.liquify;
                egui::ComboBox::from_id_salt("liquify-mode")
                    .selected_text(brush.mode.label())
                    .show_ui(ui, |ui| {
                        for mode in [
                            plume_core::liquify::LiquifyMode::Push,
                            plume_core::liquify::LiquifyMode::Pull,
                            plume_core::liquify::LiquifyMode::Twirl,
                            plume_core::liquify::LiquifyMode::Pinch,
                            plume_core::liquify::LiquifyMode::Bloat,
                            plume_core::liquify::LiquifyMode::Smooth,
                        ] {
                            ui.selectable_value(&mut brush.mode, mode, mode.label());
                        }
                    });
                ui.add(egui::Slider::new(&mut brush.radius_px, 8.0..=320.0).text("Radius (px)"));
                ui.add(egui::Slider::new(&mut brush.strength, 0.02..=2.0).text("Strength"));
                ui.add(egui::Slider::new(&mut brush.falloff, 0.0..=1.0).text("Falloff"));
                ui.add(
                    egui::Slider::new(&mut brush.pressure_radius, 0.0..=1.0)
                        .text("Pressure → radius"),
                );
                ui.add(
                    egui::Slider::new(&mut brush.pressure_strength, 0.0..=1.0)
                        .text("Pressure → strength"),
                );
            }
            Tool::Select => {
                ui.label(
                    RichText::new("Click a curve or guide. Drag on empty space for a marquee. Hold Shift to add.")
                        .size(11.0)
                        .color(theme::MUTED),
                );
                let count = self.doc.scene.selection.count();
                theme::pill(
                    ui,
                    format!("{count} selected"),
                    if count > 0 {
                        theme::CORAL
                    } else {
                        theme::FAINT
                    },
                );
                ui.add_space(4.0);
                if ui.button("Select every visible curve").clicked() {
                    let ids: Vec<CurveId> = self
                        .doc
                        .scene
                        .curves
                        .iter()
                        .filter(|c| {
                            self.doc.scene.curve_visible(c) && !self.doc.scene.curve_locked(c)
                        })
                        .map(|c| c.id)
                        .collect();
                    self.doc.scene.selection.clear();
                    for id in ids {
                        self.doc.scene.selection.curves.insert(id);
                    }
                    self.needs_render = true;
                }
                if ui.button("Clear selection").clicked() {
                    self.doc.scene.selection.clear();
                    self.needs_render = true;
                }
            }
            Tool::Move => {
                ui.label(
                    RichText::new("Transform mode")
                        .size(11.0)
                        .color(theme::MUTED),
                );
                ui.horizontal(|ui| {
                    for mode in [
                        TransformMode::Translate,
                        TransformMode::Rotate,
                        TransformMode::Scale,
                    ] {
                        if ui
                            .selectable_label(self.settings.transform_mode == mode, mode.label())
                            .clicked()
                        {
                            self.settings.transform_mode = mode;
                        }
                    }
                });
                ui.checkbox(&mut self.settings.move_plane, "Drag on the camera plane");
                snap_combo(ui, "Snap", &mut self.settings.snap);
                theme::divider(ui);
                for (label, action) in [
                    ("Duplicate", 0),
                    ("Mirror", 1),
                    ("Group", 2),
                    ("Ungroup", 3),
                    ("Delete", 4),
                ] {
                    if ui.button(label).clicked() {
                        match action {
                            0 => self.duplicate_selection(),
                            1 => self.mirror_selection(),
                            2 => self.group_selection(),
                            3 => self.ungroup_selection(),
                            _ => self.delete_selection(),
                        }
                    }
                }
            }
            Tool::Guide => {
                ui.label(
                    RichText::new("Click a surface to drop a construction plane. Choose a primitive below for a new guide.")
                        .size(11.0)
                        .color(theme::MUTED),
                );
                self.guide_primitive_buttons(ui);
            }
            Tool::Line | Tool::Circle | Tool::Ellipse | Tool::Polygon => {
                if self.tool == Tool::Polygon {
                    ui.add(
                        egui::Slider::new(&mut self.settings.shape_sides, 3..=32)
                            .text("Sides")
                            .step_by(1.0),
                    );
                }
                ui.checkbox(&mut self.settings.constrain, "Constrain proportions");
                ui.checkbox(&mut self.settings.draw_on_guide, "Project onto guides");
                snap_combo(ui, "Snap", &mut self.settings.snap);
            }
        }

        theme::divider(ui);
        theme::section_label(ui, "Active brush");
        let brush = self.current_brush();
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 54.0), Sense::click());
        ui.painter().rect(
            rect,
            CornerRadius::same(7),
            theme::PANEL_ALT,
            Stroke::new(1.0, theme::LINE_SOFT),
            StrokeKind::Inside,
        );
        theme::brush_preview(
            ui.painter(),
            rect.shrink2(Vec2::new(4.0, 4.0)),
            &brush,
            false,
        );
        if response.clicked() {
            self.left_tab = LeftTab::Brushes;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(&brush.name).size(11.0).color(theme::IVORY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.1} mm", self.settings.size))
                        .size(10.0)
                        .color(theme::FAINT),
                );
            });
        });
        ui.label(
            RichText::new(brush.category.label())
                .size(10.0)
                .color(theme::FAINT),
        );
    }

    fn brushes_panel(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "Brush library");
        ui.add(
            egui::TextEdit::singleline(&mut self.brush_search)
                .hint_text("Search brushes and categories")
                .desired_width(ui.available_width()),
        );

        let brushes: Vec<Brush> = self
            .library
            .search(&self.brush_search)
            .into_iter()
            .cloned()
            .collect();
        ui.add_space(4.0);
        theme::pill(
            ui,
            format!("{} brushes", brushes.len()),
            if brushes.is_empty() {
                theme::RED
            } else {
                theme::TEAL
            },
        );
        ui.add_space(4.0);

        let mut last_category: Option<BrushCategory> = None;
        for brush in &brushes {
            if last_category != Some(brush.category) {
                if last_category.is_some() {
                    ui.add_space(4.0);
                }
                ui.horizontal(|ui| {
                    theme::category_dot(ui, category_color(brush.category));
                    ui.label(
                        RichText::new(brush.category.label())
                            .size(9.5)
                            .strong()
                            .color(theme::FAINT),
                    );
                });
                last_category = Some(brush.category);
            }
            if theme::brush_chip(ui, brush, self.brush_id == brush.id, true) {
                self.select_brush(&brush.id);
            }
        }
        if brushes.is_empty() {
            ui.label(
                RichText::new("No brush matches that search.")
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
        theme::divider(ui);
        if ui.button("Restore default library").clicked() {
            self.library = plume_core::brush::BrushLibrary::default();
            self.select_brush("sketch-medium");
        }
    }

    fn guide_primitive_buttons(&mut self, ui: &mut egui::Ui) {
        let entries: [(GuideKind, &str); 6] = [
            (
                GuideKind::Plane {
                    width: 500.0,
                    height: 500.0,
                },
                "Plane",
            ),
            (
                GuideKind::Box {
                    size: Vec3::new(200.0, 200.0, 200.0),
                },
                "Box",
            ),
            (
                GuideKind::Sphere {
                    radius: 140.0,
                    segments: 48,
                    rings: 28,
                },
                "Sphere",
            ),
            (
                GuideKind::Cylinder {
                    radius: 120.0,
                    height: 260.0,
                    segments: 48,
                },
                "Cylinder",
            ),
            (
                GuideKind::Cone {
                    radius: 140.0,
                    height: 260.0,
                    segments: 48,
                },
                "Cone",
            ),
            (
                GuideKind::Torus {
                    major_radius: 160.0,
                    minor_radius: 52.0,
                    major_segments: 60,
                    minor_segments: 20,
                },
                "Torus",
            ),
        ];
        for pair in entries.chunks(3) {
            ui.horizontal(|ui| {
                for (kind, name) in pair {
                    if ui
                        .add_sized([64.0, 28.0], egui::Button::new(*name))
                        .clicked()
                    {
                        self.add_default_guide((*kind).clone());
                    }
                }
            });
        }
        if ui.button("Import mesh as guide…").clicked() {
            self.import_mesh_dialog();
        }
    }

    fn guides_panel(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "New guide");
        self.guide_primitive_buttons(ui);
        theme::divider(ui);
        theme::section_label(ui, "Scene guides");

        #[derive(Clone)]
        struct GuideRow {
            id: GuideId,
            name: String,
            kind: String,
            active: bool,
            visible: bool,
            locked: bool,
            selected: bool,
        }
        let rows: Vec<GuideRow> = self
            .doc
            .scene
            .guides
            .iter()
            .map(|g| GuideRow {
                id: g.id,
                name: g.name.clone(),
                kind: g.kind.label().to_string(),
                active: g.active,
                visible: g.visible,
                locked: g.locked,
                selected: self.doc.scene.selection.guides.contains(&g.id),
            })
            .collect();

        if rows.is_empty() {
            ui.label(
                RichText::new(
                    "No guides yet. Add a plane or primitive to start sketching in space.",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
        }

        for row in rows {
            Frame::new()
                .fill(if row.active || row.selected {
                    theme::PANEL_HIGH
                } else {
                    theme::PANEL_ALT
                })
                .inner_margin(egui::Margin::symmetric(7, 5))
                .corner_radius(CornerRadius::same(6))
                .stroke(Stroke::new(
                    1.0,
                    if row.active {
                        theme::TEAL.gamma_multiply(0.8)
                    } else {
                        theme::LINE_SOFT
                    },
                ))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let active = icon_button(
                            ui,
                            theme::Icon::Sparkle,
                            row.active,
                            "Use as active guide",
                        );
                        if active.clicked() {
                            self.doc.scene.set_active_guide(if row.active {
                                None
                            } else {
                                Some(row.id)
                            });
                            self.touch_document();
                            self.needs_render = true;
                        }
                        let label =
                            ui.selectable_label(row.selected, RichText::new(&row.name).size(11.0));
                        if label.clicked() {
                            self.doc.scene.selection.only_guide(row.id);
                            self.doc.scene.set_active_guide(Some(row.id));
                            self.needs_render = true;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let eye = icon_button(
                                ui,
                                if row.visible {
                                    theme::Icon::Eye
                                } else {
                                    theme::Icon::EyeOff
                                },
                                false,
                                "Show or hide guide",
                            );
                            if eye.clicked() {
                                self.record("Guide visibility");
                                if let Some(g) =
                                    self.doc.scene.guides.iter_mut().find(|g| g.id == row.id)
                                {
                                    g.visible = !g.visible;
                                }
                                self.touch_document();
                                self.needs_render = true;
                            }
                            let lock = icon_button(
                                ui,
                                if row.locked {
                                    theme::Icon::Lock
                                } else {
                                    theme::Icon::Unlock
                                },
                                row.locked,
                                "Lock guide",
                            );
                            if lock.clicked() {
                                self.record("Guide lock");
                                if let Some(g) =
                                    self.doc.scene.guides.iter_mut().find(|g| g.id == row.id)
                                {
                                    g.locked = !g.locked;
                                }
                                self.touch_document();
                            }
                            ui.label(RichText::new(&row.kind).size(9.0).color(theme::FAINT));
                        });
                    });
                });
            ui.add_space(3.0);
        }

        theme::divider(ui);
        theme::section_label(ui, "Symmetry");
        let mut mirror = self.doc.scene.mirror.clone();
        let mut changed = ui
            .checkbox(&mut mirror.enabled, "Mirror new strokes")
            .changed();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Axis").size(11.0).color(theme::MUTED));
            for axis in [MirrorAxis::X, MirrorAxis::Y, MirrorAxis::Z] {
                if ui
                    .selectable_label(mirror.axis == axis, axis.label())
                    .clicked()
                {
                    mirror.axis = axis;
                    changed = true;
                }
            }
        });
        changed |= ui
            .add(egui::Slider::new(&mut mirror.offset, -500.0..=500.0).text("Plane offset"))
            .changed();
        changed |= ui
            .checkbox(&mut mirror.live, "Mirror while drawing")
            .changed();
        changed |= ui
            .checkbox(&mut mirror.mirror_guides, "Mirror guides")
            .changed();
        if changed {
            self.doc.scene.mirror = mirror;
            self.note_ui_edit("Symmetry");
            self.needs_render = true;
        }
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "Scene layers");
        let stats = self.doc.stats();
        theme::pill(
            ui,
            format!("{} curves · {} groups", stats.curves, stats.groups),
            theme::TEAL,
        );
        ui.add_space(4.0);

        #[derive(Clone)]
        struct GroupRow {
            id: plume_core::GroupId,
            name: String,
            visible: bool,
            locked: bool,
        }
        let groups: Vec<GroupRow> = self
            .doc
            .scene
            .groups
            .iter()
            .map(|g| GroupRow {
                id: g.id,
                name: g.name.clone(),
                visible: g.visible,
                locked: g.locked,
            })
            .collect();

        for group in &groups {
            Frame::new()
                .fill(theme::PANEL_ALT)
                .inner_margin(egui::Margin::symmetric(7, 4))
                .corner_radius(CornerRadius::same(5))
                .stroke(Stroke::new(1.0, theme::LINE_SOFT))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icon_button(
                            ui,
                            if group.visible {
                                theme::Icon::Eye
                            } else {
                                theme::Icon::EyeOff
                            },
                            false,
                            "Group visibility",
                        )
                        .clicked()
                        {
                            self.record("Group visibility");
                            if let Some(g) =
                                self.doc.scene.groups.iter_mut().find(|g| g.id == group.id)
                            {
                                g.visible = !g.visible;
                            }
                            self.touch_document();
                            self.needs_render = true;
                        }
                        if icon_button(
                            ui,
                            if group.locked {
                                theme::Icon::Lock
                            } else {
                                theme::Icon::Unlock
                            },
                            group.locked,
                            "Group lock",
                        )
                        .clicked()
                        {
                            self.record("Group lock");
                            if let Some(g) =
                                self.doc.scene.groups.iter_mut().find(|g| g.id == group.id)
                            {
                                g.locked = !g.locked;
                            }
                            self.touch_document();
                        }
                        ui.label(
                            RichText::new(&group.name)
                                .size(11.0)
                                .strong()
                                .color(theme::IVORY),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_button(ui, theme::Icon::Trash, false, "Ungroup").clicked() {
                                self.doc.scene.selection.groups.clear();
                                self.doc.scene.selection.groups.insert(group.id);
                                self.ungroup_selection();
                            }
                            if icon_button(ui, theme::Icon::Plus, false, "Select group").clicked() {
                                self.doc.scene.selection.clear();
                                self.doc.scene.selection.groups.insert(group.id);
                                for curve in &self.doc.scene.curves {
                                    if curve.group == Some(group.id) {
                                        self.doc.scene.selection.curves.insert(curve.id);
                                    }
                                }
                                self.needs_render = true;
                            }
                        });
                    });
                });
            ui.add_space(2.0);

            let curves: Vec<(CurveId, String, bool, bool, bool)> = self
                .doc
                .scene
                .curves
                .iter()
                .filter(|c| c.group == Some(group.id))
                .map(|c| {
                    (
                        c.id,
                        c.name.clone(),
                        c.visible,
                        c.locked,
                        self.doc.scene.selection.curves.contains(&c.id),
                    )
                })
                .collect();
            for (id, name, visible, locked, selected) in curves {
                self.curve_layer_row(
                    ui,
                    id,
                    &name,
                    LayerRowState {
                        visible,
                        locked,
                        selected,
                        indented: true,
                    },
                );
            }
        }

        let ungrouped: Vec<(CurveId, String, bool, bool, bool)> = self
            .doc
            .scene
            .curves
            .iter()
            .filter(|c| c.group.is_none())
            .map(|c| {
                (
                    c.id,
                    c.name.clone(),
                    c.visible,
                    c.locked,
                    self.doc.scene.selection.curves.contains(&c.id),
                )
            })
            .collect();
        if !ungrouped.is_empty() {
            if !groups.is_empty() {
                theme::section_label(ui, "Ungrouped");
            }
            for (id, name, visible, locked, selected) in ungrouped {
                self.curve_layer_row(
                    ui,
                    id,
                    &name,
                    LayerRowState {
                        visible,
                        locked,
                        selected,
                        indented: false,
                    },
                );
            }
        }

        if self.doc.scene.curves.is_empty() {
            ui.label(
                RichText::new("The scene is empty. Pick a brush and draw on the ground plane, or open the gallery.")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            if ui.button("Open gallery").clicked() {
                self.dialog = Dialog::Gallery;
            }
        }

        theme::divider(ui);
        ui.horizontal(|ui| {
            if ui.button("Group selection").clicked() {
                self.group_selection();
            }
            if ui.button("Duplicate").clicked() {
                self.duplicate_selection();
            }
        });
        if ui.button("Select all visible").clicked() {
            let ids: Vec<CurveId> = self
                .doc
                .scene
                .curves
                .iter()
                .filter(|c| self.doc.scene.curve_visible(c))
                .map(|c| c.id)
                .collect();
            self.doc.scene.selection.clear();
            for id in ids {
                self.doc.scene.selection.curves.insert(id);
            }
            self.needs_render = true;
        }
    }

    fn curve_layer_row(
        &mut self,
        ui: &mut egui::Ui,
        id: CurveId,
        name: &str,
        state: LayerRowState,
    ) {
        let LayerRowState {
            visible,
            locked,
            selected,
            indented,
        } = state;
        ui.horizontal(|ui| {
            if indented {
                ui.add_space(10.0);
            }
            if icon_button(
                ui,
                if visible {
                    theme::Icon::Eye
                } else {
                    theme::Icon::EyeOff
                },
                false,
                "Curve visibility",
            )
            .clicked()
            {
                self.record("Curve visibility");
                if let Some(c) = self.doc.scene.curve_mut(id) {
                    c.visible = !c.visible;
                }
                self.touch_document();
                self.needs_render = true;
            }
            if icon_button(
                ui,
                if locked {
                    theme::Icon::Lock
                } else {
                    theme::Icon::Unlock
                },
                locked,
                "Curve lock",
            )
            .clicked()
            {
                self.record("Curve lock");
                if let Some(c) = self.doc.scene.curve_mut(id) {
                    c.locked = !c.locked;
                }
                self.touch_document();
            }
            let response = ui.selectable_label(selected, RichText::new(name).size(11.0));
            if response.clicked() {
                let shift = ui.input(|i| i.modifiers.shift);
                if shift {
                    self.doc.scene.selection.toggle_curve(id);
                } else {
                    self.doc.scene.selection.only_curve(id);
                }
                self.needs_render = true;
            }
            if response.double_clicked() {
                self.frame_selection();
            }
        });
    }
}

// -- small shared widgets ---------------------------------------------------

fn icon_button(ui: &mut egui::Ui, kind: theme::Icon, selected: bool, hint: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(25.0), Sense::click());
    let hovered = response.hovered();
    let bg = if selected {
        theme::CORAL.gamma_multiply(0.24)
    } else if hovered {
        theme::PANEL_HIGH
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(5),
        bg,
        Stroke::new(
            1.0,
            if selected {
                theme::CORAL.gamma_multiply(0.7)
            } else {
                Color32::TRANSPARENT
            },
        ),
        StrokeKind::Inside,
    );
    theme::icon(
        ui.painter(),
        rect.shrink(5.5),
        kind,
        if selected || hovered {
            theme::IVORY
        } else {
            theme::MUTED
        },
    );
    response.on_hover_text(hint)
}

fn snap_combo(ui: &mut egui::Ui, label: &str, snap: &mut SnapMode) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(11.0).color(theme::MUTED));
        egui::ComboBox::from_id_salt(format!("snap-{label}"))
            .selected_text(snap.label())
            .width(ui.available_width().min(130.0))
            .show_ui(ui, |ui| {
                for mode in [
                    SnapMode::Off,
                    SnapMode::Grid,
                    SnapMode::Angle15,
                    SnapMode::Angle45,
                ] {
                    ui.selectable_value(snap, mode, mode.label());
                }
            });
    });
}

fn category_color(category: BrushCategory) -> Color32 {
    match category {
        BrushCategory::Sketch | BrushCategory::Ink => theme::IVORY,
        BrushCategory::Marker => theme::CORAL,
        BrushCategory::Calligraphy => theme::GOLD,
        BrushCategory::Wide => theme::TEAL,
        BrushCategory::Dots | BrushCategory::Stipple => theme::LILAC,
        BrushCategory::Hatching | BrushCategory::Halftone => theme::BLUE,
        BrushCategory::Terrazzo => theme::GREEN,
        BrushCategory::Smooth => theme::TEAL,
        BrushCategory::Rough => theme::GOLD,
        BrushCategory::Taper => theme::CORAL,
        BrushCategory::Experimental => theme::RED,
    }
}

// === INSPECTOR ===

impl PlumeApp {
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().slider_width = 104.0;
        ui.spacing_mut().item_spacing.x = 5.0;
        ui.horizontal(|ui| {
            theme::heading(ui, "Inspector", None);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let stats = self.doc.stats();
                theme::pill(ui, format!("{} pts", stats.control_points), theme::FAINT);
            });
        });
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for tab in InspectorTab::ALL {
                let selected = self.inspector == tab;
                let response = ui.add(
                    egui::Button::new(RichText::new(tab.label()).size(10.5).color(if selected {
                        theme::IVORY
                    } else {
                        theme::MUTED
                    }))
                    .fill(if selected {
                        theme::PANEL_HIGH
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(
                        1.0,
                        if selected {
                            theme::LINE
                        } else {
                            Color32::TRANSPARENT
                        },
                    ))
                    .corner_radius(CornerRadius::same(5))
                    .min_size(Vec2::new(66.0, 24.0)),
                );
                if response.clicked() {
                    self.inspector = tab;
                }
            }
        });
        theme::divider(ui);
        egui::ScrollArea::vertical()
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.inspector {
                InspectorTab::Style => self.style_inspector(ui),
                InspectorTab::Surface => self.surface_inspector(ui),
                InspectorTab::Render => self.render_inspector(ui),
                InspectorTab::Motion => self.motion_inspector(ui),
            });
    }

    // -- Style --------------------------------------------------------------

    fn style_inspector(&mut self, ui: &mut egui::Ui) {
        let selection = self.doc.scene.selection.clone();
        let first_curve = selection.curves.iter().next().copied();

        if let Some(id) = first_curve {
            if let Some(mut curve) = self.doc.scene.curve(id).cloned() {
                theme::section_label(ui, "Selected stroke");
                if selection.curves.len() > 1 {
                    ui.label(
                        RichText::new(format!(
                            "{} strokes selected. Editing the first.",
                            selection.curves.len()
                        ))
                        .size(10.5)
                        .color(theme::GOLD),
                    );
                }
                let mut changed = false;
                let mut label = "Stroke style";
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut curve.name)
                                .desired_width(ui.available_width() - 60.0),
                        )
                        .changed();
                    theme::pill(ui, curve.kind.label(), theme::TEAL);
                });
                ui.label(
                    RichText::new(format!(
                        "{} control points · {:.1} mm long",
                        curve.points.len(),
                        curve.sample(1.0).arc_length
                    ))
                    .size(10.0)
                    .color(theme::FAINT),
                );

                changed |= theme::color_edit(ui, "Stroke colour", &mut curve.color);
                if changed {
                    label = "Stroke colour";
                }
                changed |= ui
                    .add(
                        egui::Slider::new(&mut curve.tension, 0.0..=1.0)
                            .text("Tension")
                            .step_by(0.01),
                    )
                    .changed();
                if ui.checkbox(&mut curve.closed, "Closed loop").changed() {
                    changed = true;
                    label = "Close stroke";
                }
                if ui
                    .checkbox(&mut curve.cutout, "Cut out pattern in 3D")
                    .changed()
                {
                    changed = true;
                    label = "Stroke cutout";
                }
                if ui.checkbox(&mut curve.visible, "Visible").changed() {
                    changed = true;
                    label = "Stroke visibility";
                }

                theme::divider(ui);
                theme::section_label(ui, "Stroke brush");
                egui::ComboBox::from_id_salt("curve-profile")
                    .selected_text(curve.brush.profile.shape.label())
                    .show_ui(ui, |ui| {
                        for shape in [
                            ProfileShape::Round,
                            ProfileShape::Flat,
                            ProfileShape::Tapered,
                            ProfileShape::Custom,
                        ] {
                            if ui
                                .selectable_value(
                                    &mut curve.brush.profile.shape,
                                    shape,
                                    shape.label(),
                                )
                                .clicked()
                            {
                                changed = true;
                                label = "Stroke profile";
                            }
                        }
                    });
                if curve.brush.profile.shape != ProfileShape::Custom {
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut curve.brush.profile.sides, 3..=64).text("Sides"),
                        )
                        .changed();
                }
                if curve.brush.profile.shape == ProfileShape::Flat {
                    changed |= ui
                        .add(
                            egui::Slider::new(&mut curve.brush.profile.aspect, 0.05..=1.0)
                                .text("Flatness"),
                        )
                        .changed();
                }
                changed |= ui
                    .add(
                        egui::Slider::new(&mut curve.brush.profile.rotation_deg, -180.0..=180.0)
                            .text("Profile roll")
                            .suffix("°"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut curve.brush.emission, 0.0..=4.0).text("Emission"))
                    .changed();
                if changed {
                    if let Some(target) = self.doc.scene.curve_mut(id) {
                        target.name = curve.name.clone();
                        target.color = curve.color;
                        target.tension = curve.tension;
                        target.closed = curve.closed;
                        target.cutout = curve.cutout;
                        target.visible = curve.visible;
                        target.brush = curve.brush.clone();
                        target.refresh_frames();
                    }
                    self.note_ui_edit(label);
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Duplicate").clicked() {
                        self.duplicate_selection();
                    }
                    if ui.button("Frame").clicked() {
                        self.frame_selection();
                    }
                    if ui.button("Mirror").clicked() {
                        self.mirror_selection();
                    }
                    if ui.button("Delete").clicked() {
                        self.delete_selection();
                    }
                });
            }
            theme::divider(ui);
        } else if !selection.guides.is_empty() {
            theme::section_label(ui, "Selected guide");
            ui.label(
                RichText::new("Shape, opacity and placement live in the Surface tab.")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            if ui.button("Edit guide surface").clicked() {
                self.inspector = InspectorTab::Surface;
            }
            theme::divider(ui);
        }

        self.brush_style_ui(ui);
    }

    fn brush_style_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut brush) = self.current_brush_ref().cloned() else {
            return;
        };
        theme::section_label(ui, "Active brush");
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 58.0), Sense::hover());
        ui.painter().rect(
            rect,
            CornerRadius::same(7),
            theme::PANEL_ALT,
            Stroke::new(1.0, theme::LINE_SOFT),
            StrokeKind::Inside,
        );
        theme::brush_preview(
            ui.painter(),
            rect.shrink2(Vec2::new(5.0, 5.0)),
            &brush,
            false,
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new(&brush.name).size(11.5).color(theme::IVORY));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(brush.category.label())
                        .size(9.5)
                        .color(theme::FAINT),
                );
            });
        });

        let mut changed = false;
        let mut label = "Brush";
        changed |= theme::color_edit(ui, "Ink colour", &mut brush.color);
        if changed {
            label = "Brush colour";
        }
        let mut opacity = brush.opacity;
        if ui
            .add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity"))
            .changed()
        {
            brush.opacity = opacity;
            changed = true;
            label = "Brush opacity";
        }
        if ui
            .add(egui::Slider::new(&mut brush.emission, 0.0..=4.0).text("Emission"))
            .changed()
        {
            changed = true;
            label = "Brush emission";
        }
        if ui
            .add(egui::Slider::new(&mut brush.taper.start, 0.0..=0.9).text("Taper in"))
            .changed()
        {
            changed = true;
            label = "Brush taper";
        }
        if ui
            .add(egui::Slider::new(&mut brush.taper.end, 0.0..=0.9).text("Taper out"))
            .changed()
        {
            changed = true;
            label = "Brush taper";
        }
        if ui
            .add(egui::Slider::new(&mut brush.taper.tip, 0.0..=0.95).text("Tip size"))
            .changed()
        {
            changed = true;
            label = "Brush taper";
        }
        if ui
            .add(egui::Slider::new(&mut brush.taper.pressure, 0.0..=1.0).text("Tip pressure"))
            .changed()
        {
            changed = true;
            label = "Brush taper";
        }

        theme::section_label(ui, "Cross-section");
        egui::ComboBox::from_id_salt("active-profile")
            .selected_text(brush.profile.shape.label())
            .show_ui(ui, |ui| {
                for shape in [
                    ProfileShape::Round,
                    ProfileShape::Flat,
                    ProfileShape::Tapered,
                    ProfileShape::Custom,
                ] {
                    if ui
                        .selectable_value(&mut brush.profile.shape, shape, shape.label())
                        .clicked()
                    {
                        changed = true;
                        label = "Brush profile";
                    }
                }
            });
        if brush.profile.shape != ProfileShape::Custom
            && ui
                .add(egui::Slider::new(&mut brush.profile.sides, 3..=64).text("Sides"))
                .changed()
        {
            changed = true;
            label = "Brush profile";
        }
        if brush.profile.shape == ProfileShape::Flat
            && ui
                .add(egui::Slider::new(&mut brush.profile.aspect, 0.05..=1.0).text("Flatness"))
                .changed()
        {
            changed = true;
            label = "Brush profile";
        }
        if ui
            .add(
                egui::Slider::new(&mut brush.profile.rotation_deg, -180.0..=180.0)
                    .text("Roll")
                    .suffix("°"),
            )
            .changed()
        {
            changed = true;
            label = "Brush profile";
        }

        theme::section_label(ui, "Pattern");
        egui::ComboBox::from_id_salt("pattern-kind")
            .selected_text(brush.pattern.kind.label())
            .show_ui(ui, |ui| {
                for kind in [
                    PatternKind::None,
                    PatternKind::Halftone,
                    PatternKind::Hatch,
                    PatternKind::CrossHatch,
                    PatternKind::Stipple,
                    PatternKind::Terrazzo,
                    PatternKind::Custom,
                ] {
                    if ui
                        .selectable_value(&mut brush.pattern.kind, kind, kind.label())
                        .clicked()
                    {
                        changed = true;
                        label = "Brush pattern";
                    }
                }
            });
        if brush.pattern.kind != PatternKind::None {
            if ui
                .add(
                    egui::Slider::new(&mut brush.pattern.scale_u, 0.5..=200.0)
                        .text("Repeat along")
                        .logarithmic(true),
                )
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            if ui
                .add(
                    egui::Slider::new(&mut brush.pattern.scale_v, 0.5..=64.0)
                        .text("Repeat around")
                        .logarithmic(true),
                )
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            if ui
                .add(egui::Slider::new(&mut brush.pattern.spacing, 0.1..=3.0).text("Cell spacing"))
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            if ui
                .add(
                    egui::Slider::new(&mut brush.pattern.thickness, 0.02..=0.95)
                        .text("Ink coverage"),
                )
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            if ui
                .add(
                    egui::Slider::new(&mut brush.pattern.angle_deg, -180.0..=180.0)
                        .text("Angle")
                        .suffix("°"),
                )
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            if ui
                .add(egui::Slider::new(&mut brush.pattern.density, 0.0..=1.0).text("Density"))
                .changed()
            {
                changed = true;
                label = "Brush pattern";
            }
            egui::ComboBox::from_id_salt("pattern-channel")
                .selected_text(match brush.pattern.channel {
                    PatternChannel::Alpha => "Cutout alpha",
                    PatternChannel::Color => "Secondary colour",
                    PatternChannel::Both => "Alpha + colour",
                })
                .show_ui(ui, |ui| {
                    for channel in [
                        PatternChannel::Alpha,
                        PatternChannel::Color,
                        PatternChannel::Both,
                    ] {
                        let name = match channel {
                            PatternChannel::Alpha => "Cutout alpha",
                            PatternChannel::Color => "Secondary colour",
                            PatternChannel::Both => "Alpha + colour",
                        };
                        if ui
                            .selectable_value(&mut brush.pattern.channel, channel, name)
                            .clicked()
                        {
                            changed = true;
                            label = "Brush pattern channel";
                        }
                    }
                });
            changed |= theme::color_edit(ui, "Second ink", &mut brush.pattern.color_b);
        }

        if changed {
            if let Some(target) = self
                .library
                .brushes
                .iter_mut()
                .find(|candidate| candidate.id == brush.id)
            {
                *target = brush.clone();
            }
            self.note_ui_edit(label);
            self.needs_render = true;
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Duplicate brush").clicked() {
                if let Some(id) = self.library.duplicate(&self.brush_id) {
                    self.select_brush(&id);
                }
            }
            let builtin = self
                .library
                .get(&self.brush_id)
                .map(|b| b.builtin)
                .unwrap_or(true);
            if ui
                .add_enabled(!builtin, egui::Button::new("Remove"))
                .clicked()
            {
                self.library.remove(&self.brush_id);
                self.select_brush("sketch-medium");
            }
        });
        ui.label(
            RichText::new(
                "Size, pressure and smoothing for the current stroke live in the Tools tab.",
            )
            .size(9.5)
            .color(theme::FAINT),
        );
    }
}

// === SURFACE ===

impl PlumeApp {
    fn surface_inspector(&mut self, ui: &mut egui::Ui) {
        let selection = self.doc.scene.selection.clone();
        let guide_id = selection
            .guides
            .iter()
            .next()
            .copied()
            .or(self.doc.scene.active_guide);

        if let Some(id) = guide_id {
            if let Some(mut guide) = self.doc.scene.guide(id).cloned() {
                theme::section_label(ui, "Construction surface");
                let mut changed = false;
                let mut label = "Guide";
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut guide.name)
                            .desired_width(ui.available_width()),
                    )
                    .changed()
                {
                    changed = true;
                    label = "Rename guide";
                }
                ui.horizontal(|ui| {
                    theme::pill(ui, guide.kind.label(), theme::TEAL);
                    if guide.active {
                        theme::pill(ui, "Active", theme::CORAL);
                    }
                    if guide.locked {
                        theme::pill(ui, "Locked", theme::GOLD);
                    }
                });
                changed |= ui
                    .add(egui::Slider::new(&mut guide.opacity, 0.0..=1.0).text("Opacity"))
                    .changed();
                changed |= theme::color_edit(ui, "Guide colour", &mut guide.color);
                if ui.checkbox(&mut guide.solid, "Shaded surface").changed() {
                    changed = true;
                    label = "Guide shaded";
                }
                if ui.checkbox(&mut guide.visible, "Visible").changed() {
                    changed = true;
                    label = "Guide visibility";
                }
                if ui.checkbox(&mut guide.locked, "Lock placement").changed() {
                    changed = true;
                    label = "Guide lock";
                }

                theme::section_label(ui, "Dimensions");
                match &mut guide.kind {
                    GuideKind::Plane { width, height } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(width, 20.0..=4000.0)
                                    .text("Width")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(height, 20.0..=4000.0)
                                    .text("Height")
                                    .logarithmic(true),
                            )
                            .changed();
                    }
                    GuideKind::Box { size } => {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut size.x)
                                    .speed(1.0)
                                    .range(1.0..=5000.0)
                                    .prefix("X "),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut size.y)
                                    .speed(1.0)
                                    .range(1.0..=5000.0)
                                    .prefix("Y "),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut size.z)
                                    .speed(1.0)
                                    .range(1.0..=5000.0)
                                    .prefix("Z "),
                            )
                            .changed();
                    }
                    GuideKind::Sphere {
                        radius,
                        segments,
                        rings,
                    } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(radius, 1.0..=2000.0)
                                    .text("Radius")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(segments, 8..=128).text("Segments"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(rings, 4..=96).text("Rings"))
                            .changed();
                    }
                    GuideKind::Cylinder {
                        radius,
                        height,
                        segments,
                    }
                    | GuideKind::Cone {
                        radius,
                        height,
                        segments,
                    } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(radius, 1.0..=2000.0)
                                    .text("Radius")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(height, 1.0..=5000.0)
                                    .text("Height")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(segments, 8..=128).text("Segments"))
                            .changed();
                    }
                    GuideKind::Torus {
                        major_radius,
                        minor_radius,
                        major_segments,
                        minor_segments,
                    } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(major_radius, 2.0..=2000.0)
                                    .text("Major radius")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(minor_radius, 0.5..=1000.0)
                                    .text("Minor radius")
                                    .logarithmic(true),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(major_segments, 8..=160).text("Major segments"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(minor_segments, 4..=64).text("Minor segments"))
                            .changed();
                    }
                    GuideKind::Freeform { rows, subdivisions } => {
                        let points: usize = rows.iter().map(|row| row.len()).sum();
                        ui.label(
                            RichText::new(format!(
                                "{points} freeform points · {subdivisions}× subdivision"
                            ))
                            .size(10.5)
                            .color(theme::MUTED),
                        );
                    }
                    GuideKind::Loft {
                        rows,
                        source_curves,
                    } => {
                        ui.label(
                            RichText::new(format!(
                                "{} lofted rows · {} source curves",
                                rows.len(),
                                source_curves.len()
                            ))
                            .size(10.5)
                            .color(theme::MUTED),
                        );
                    }
                    GuideKind::Imported { mesh, .. } => {
                        ui.label(
                            RichText::new(format!(
                                "Imported mesh · {} triangles",
                                mesh.triangle_count()
                            ))
                            .size(10.5)
                            .color(theme::MUTED),
                        );
                    }
                }

                theme::section_label(ui, "Placement");
                let mut position = guide.transform.w_axis.truncate();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut position.x)
                            .speed(1.0)
                            .prefix("X ")
                            .suffix(" mm"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut position.y)
                            .speed(1.0)
                            .prefix("Y ")
                            .suffix(" mm"),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut position.z)
                            .speed(1.0)
                            .prefix("Z ")
                            .suffix(" mm"),
                    )
                    .changed();
                guide.transform.w_axis = position.extend(1.0);

                if changed {
                    if let Some(target) = self.doc.scene.guides.iter_mut().find(|g| g.id == id) {
                        *target = guide.clone();
                        target.rebuild();
                    }
                    self.note_ui_edit(label);
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button("Activate").clicked() {
                        self.record("Activate guide");
                        self.doc.scene.set_active_guide(Some(id));
                        self.touch_document();
                        self.needs_render = true;
                    }
                    if ui.button("Frame").clicked() {
                        if let Some(target) = self.doc.scene.guides.iter_mut().find(|g| g.id == id)
                        {
                            let (min, max) = target.world_bounds();
                            let aspect =
                                (self.viewport_size.x / self.viewport_size.y.max(1.0)).max(0.1);
                            self.exit_preview();
                            self.doc.camera.frame_bounds(min, max, aspect);
                            self.needs_render = true;
                        }
                    }
                    if ui.button("Remove").clicked() {
                        self.record("Remove guide");
                        self.doc.scene.remove_guide(id);
                        self.doc.scene.selection.guides.remove(&id);
                        if self.doc.scene.active_guide == Some(id) {
                            self.doc.scene.active_guide = None;
                        }
                        self.touch_document();
                        self.needs_render = true;
                    }
                });
            }
            theme::divider(ui);
        } else {
            theme::section_label(ui, "Selected surface");
            ui.label(
                RichText::new(
                    "Select a guide in the viewport or the Guides drawer to edit its shape and placement.",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
            if ui.button("Browse guides").clicked() {
                self.left_tab = LeftTab::Guides;
            }
            theme::divider(ui);
        }

        theme::section_label(ui, "Symmetry");
        let mut mirror = self.doc.scene.mirror.clone();
        let mut mirror_changed = ui
            .checkbox(&mut mirror.enabled, "Mirror new strokes")
            .changed();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Axis").size(11.0).color(theme::MUTED));
            for axis in [MirrorAxis::X, MirrorAxis::Y, MirrorAxis::Z] {
                mirror_changed |= ui
                    .selectable_label(mirror.axis == axis, axis.label())
                    .clicked();
            }
        });
        mirror_changed |= ui
            .add(egui::Slider::new(&mut mirror.offset, -2000.0..=2000.0).text("Mirror plane"))
            .changed();
        mirror_changed |= ui
            .checkbox(&mut mirror.live, "Mirror while drawing")
            .changed();
        mirror_changed |= ui
            .checkbox(&mut mirror.mirror_guides, "Include guides")
            .changed();
        if mirror_changed {
            self.doc.scene.mirror = mirror;
            self.note_ui_edit("Symmetry settings");
        }

        theme::divider(ui);
        theme::section_label(ui, "Document");
        let mut document_changed = false;
        let mut name = self.doc.name.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut name).hint_text("Scene name"))
            .changed()
        {
            self.doc.name = name;
            document_changed = true;
        }
        let mut author = self.doc.author.clone();
        if ui
            .add(egui::TextEdit::singleline(&mut author).hint_text("Author"))
            .changed()
        {
            self.doc.author = author;
            document_changed = true;
        }
        if ui
            .add(
                egui::TextEdit::multiline(&mut self.doc.notes)
                    .hint_text("Notes, credits, production details")
                    .desired_rows(3)
                    .desired_width(ui.available_width()),
            )
            .changed()
        {
            document_changed = true;
        }
        if document_changed {
            self.note_ui_edit("Document details");
        }

        theme::divider(ui);
        theme::section_label(ui, "Construction aids");
        let mut render_changed = false;
        render_changed |= ui
            .checkbox(&mut self.doc.render.grid, "Ground grid")
            .changed();
        render_changed |= ui
            .checkbox(&mut self.doc.render.axes, "World axes")
            .changed();
        render_changed |= ui
            .checkbox(&mut self.doc.render.origin_gizmo, "Origin gizmo")
            .changed();
        render_changed |= ui
            .checkbox(&mut self.doc.render.guide_fill, "Shade guides")
            .changed();
        render_changed |= ui
            .checkbox(&mut self.doc.render.guide_wire, "Guide wireframe")
            .changed();
        if render_changed {
            self.note_ui_edit("Construction aids");
        }
    }
}

// === RENDER ===

impl PlumeApp {
    fn render_inspector(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "Viewport");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Mesh quality").size(11.0).color(theme::MUTED));
            egui::ComboBox::from_id_salt("viewport-quality")
                .selected_text(self.quality.label())
                .width(120.0)
                .show_ui(ui, |ui| {
                    for quality in [
                        MeshQuality::Draft,
                        MeshQuality::Normal,
                        MeshQuality::High,
                        MeshQuality::Export,
                    ] {
                        if ui
                            .selectable_value(&mut self.quality, quality, quality.label())
                            .clicked()
                        {
                            self.needs_render = true;
                        }
                    }
                });
        });
        if ui
            .checkbox(&mut self.show_guides, "Show construction guides")
            .changed()
        {
            self.needs_render = true;
        }
        if ui
            .checkbox(&mut self.show_stats, "Viewport statistics")
            .changed()
        {
            self.needs_render = true;
        }
        if ui
            .checkbox(&mut self.doc.render.wireframe, "Wireframe strokes")
            .changed()
        {
            self.note_ui_edit("Wireframe");
        }
        if ui
            .checkbox(&mut self.doc.render.curve_outlines, "Ink outlines")
            .changed()
        {
            self.note_ui_edit("Ink outlines");
        }
        if ui
            .checkbox(&mut self.doc.render.dynamic_resolution, "Fast navigation")
            .changed()
        {
            self.note_ui_edit("Dynamic resolution");
        }

        theme::divider(ui);
        theme::section_label(ui, "Background");
        let mut background = self.doc.render.background;
        let mut changed = false;
        egui::ComboBox::from_id_salt("background-kind")
            .selected_text(background.kind.label())
            .show_ui(ui, |ui| {
                for kind in [
                    BackgroundKind::Solid,
                    BackgroundKind::Gradient,
                    BackgroundKind::Studio,
                ] {
                    if ui
                        .selectable_value(&mut background.kind, kind, kind.label())
                        .clicked()
                    {
                        changed = true;
                    }
                }
            });
        match background.kind {
            BackgroundKind::Solid => {
                changed |= theme::color_edit(ui, "Paper colour", &mut background.color);
            }
            BackgroundKind::Gradient | BackgroundKind::Studio => {
                changed |= theme::color_edit(ui, "Top", &mut background.top);
                changed |= theme::color_edit(ui, "Bottom", &mut background.bottom);
                changed |= ui
                    .add(egui::Slider::new(&mut background.horizon, 0.0..=1.0).text("Horizon"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut background.vignette, 0.0..=1.0).text("Vignette"))
                    .changed();
            }
        }
        if changed {
            self.doc.render.background = background;
            self.note_ui_edit("Background");
        }

        let mut environment = self.doc.render.environment;
        let mut env_changed = false;
        env_changed |= ui
            .add(egui::Slider::new(&mut environment.intensity, 0.0..=3.0).text("Environment"))
            .changed();
        env_changed |= ui
            .add(
                egui::Slider::new(&mut environment.rotation_deg, -180.0..=180.0)
                    .text("Rotation")
                    .suffix("°"),
            )
            .changed();
        env_changed |= theme::color_edit(ui, "Sky bounce", &mut environment.sky_bounce);
        env_changed |= theme::color_edit(ui, "Ground bounce", &mut environment.ground_bounce);
        if env_changed {
            self.doc.render.environment = environment;
            self.note_ui_edit("Environment");
        }

        theme::divider(ui);
        theme::section_label(ui, "Lighting");
        let mut lighting = self.doc.render.lighting.clone();
        let mut lighting_changed = false;
        lighting_changed |= ui
            .add(
                egui::Slider::new(&mut lighting.exposure, -4.0..=4.0)
                    .text("Exposure")
                    .suffix(" EV"),
            )
            .changed();
        lighting_changed |= ui
            .add(egui::Slider::new(&mut lighting.ambient_intensity, 0.0..=2.0).text("Ambient"))
            .changed();
        lighting_changed |= theme::color_edit(ui, "Ambient tint", &mut lighting.ambient_color);
        lighting_changed |= ui
            .add(egui::Slider::new(&mut lighting.rim_strength, 0.0..=2.0).text("Rim"))
            .changed();
        lighting_changed |= ui
            .add(egui::Slider::new(&mut lighting.specular, 0.0..=2.0).text("Specular"))
            .changed();
        lighting_changed |= ui.checkbox(&mut lighting.shadows, "Shadows").changed();
        lighting_changed |= ui
            .checkbox(&mut lighting.studio_reflections, "Studio reflections")
            .changed();

        let mut remove_light = None;
        for light in &mut lighting.lights {
            egui::CollapsingHeader::new(
                RichText::new(format!("{}  ·  {}", light.name, light.kind.label()))
                    .size(11.0)
                    .color(if light.enabled {
                        theme::IVORY
                    } else {
                        theme::FAINT
                    }),
            )
            .id_salt(("light", light.id))
            .default_open(false)
            .show(ui, |ui| {
                lighting_changed |= ui.checkbox(&mut light.enabled, "Enabled").changed();
                egui::ComboBox::from_id_salt(("kind", light.id))
                    .selected_text(light.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in [LightKind::Directional, LightKind::Point, LightKind::Spot] {
                            if ui
                                .selectable_value(&mut light.kind, kind, kind.label())
                                .clicked()
                            {
                                lighting_changed = true;
                            }
                        }
                    });
                let mut color = light.color;
                if theme::color_edit(ui, "Colour", &mut color) {
                    light.color = color;
                    lighting_changed = true;
                }
                lighting_changed |= ui
                    .add(
                        egui::Slider::new(&mut light.intensity, 0.0..=20.0)
                            .text("Intensity")
                            .logarithmic(true),
                    )
                    .changed();
                if light.kind != LightKind::Directional {
                    lighting_changed |= ui
                        .add(
                            egui::Slider::new(&mut light.range, 10.0..=5000.0)
                                .text("Range")
                                .logarithmic(true),
                        )
                        .changed();
                }
                if light.kind == LightKind::Spot {
                    lighting_changed |= ui
                        .add(
                            egui::Slider::new(&mut light.cone_deg, 5.0..=120.0)
                                .text("Cone")
                                .suffix("°"),
                        )
                        .changed();
                }
                lighting_changed |= ui
                    .add(egui::Slider::new(&mut light.softness, 0.0..=8.0).text("Softness"))
                    .changed();
                lighting_changed |= ui.checkbox(&mut light.cast_shadow, "Cast shadow").changed();
                if ui.button("Remove light").clicked() {
                    remove_light = Some(light.id);
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("Add key light").clicked() {
                lighting.add_light(LightKind::Point);
                lighting_changed = true;
            }
            if ui.button("Reset").clicked() {
                lighting = plume_core::render::Lighting::default();
                lighting_changed = true;
            }
        });
        if let Some(id) = remove_light {
            lighting.remove_light(id);
            lighting_changed = true;
        }
        if lighting_changed {
            self.doc.render.lighting = lighting;
            self.note_ui_edit("Lighting");
        }

        theme::divider(ui);
        theme::section_label(ui, "Post-processing");
        let mut postfx = self.doc.render.postfx.clone();
        let mut post_changed = false;
        egui::ComboBox::from_id_salt("tonemap")
            .selected_text(postfx.tonemap.label())
            .show_ui(ui, |ui| {
                for tonemap in [
                    Tonemap::Linear,
                    Tonemap::Reinhard,
                    Tonemap::Aces,
                    Tonemap::Filmic,
                ] {
                    if ui
                        .selectable_value(&mut postfx.tonemap, tonemap, tonemap.label())
                        .clicked()
                    {
                        post_changed = true;
                    }
                }
            });
        post_changed |= ui
            .add(
                egui::Slider::new(&mut postfx.exposure, -4.0..=4.0)
                    .text("Post exposure")
                    .suffix(" EV"),
            )
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.saturation, 0.0..=2.0).text("Saturation"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.contrast, 0.4..=2.2).text("Contrast"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.lift, -0.3..=0.5).text("Lift"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.bloom, 0.0..=2.0).text("Bloom"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.bloom_threshold, 0.0..=2.0).text("Bloom threshold"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.vignette, 0.0..=1.5).text("Vignette"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.grain, 0.0..=0.25).text("Film grain"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.chromatic_aberration, 0.0..=1.0).text("Chromatic"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.sharpen, 0.0..=1.0).text("Sharpen"))
            .changed();
        post_changed |= ui
            .add(egui::Slider::new(&mut postfx.depth_of_field, 0.0..=1.0).text("Depth of field"))
            .changed();
        egui::ComboBox::from_id_salt("fog-mode")
            .selected_text(postfx.fog.label())
            .show_ui(ui, |ui| {
                for fog in [FogMode::None, FogMode::Linear, FogMode::Exponential] {
                    if ui
                        .selectable_value(&mut postfx.fog, fog, fog.label())
                        .clicked()
                    {
                        post_changed = true;
                    }
                }
            });
        if postfx.fog != FogMode::None {
            post_changed |= theme::color_edit(ui, "Fog colour", &mut postfx.fog_color);
            post_changed |= ui
                .add(
                    egui::Slider::new(&mut postfx.fog_density, 0.0..=0.02)
                        .text("Fog density")
                        .step_by(0.0001),
                )
                .changed();
        }
        if post_changed {
            self.doc.render.postfx = postfx;
            self.note_ui_edit("Post-processing");
        }
    }
}

// === MOTION ===

impl PlumeApp {
    fn motion_inspector(&mut self, ui: &mut egui::Ui) {
        theme::section_label(ui, "Transport");
        ui.horizontal(|ui| {
            if icon_button(ui, theme::Icon::Rewind, false, "Go to start").clicked() {
                self.stop_playback();
            }
            if icon_button(
                ui,
                if self.doc.animation.playing {
                    theme::Icon::Pause
                } else {
                    theme::Icon::Play
                },
                self.doc.animation.playing,
                "Play or pause (Space)",
            )
            .clicked()
            {
                self.toggle_playback();
            }
            if icon_button(ui, theme::Icon::Stop, false, "Stop").clicked() {
                self.stop_playback();
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{:.2} / {:.2} s",
                    self.doc.animation.time,
                    self.doc.animation.effective_duration()
                ))
                .monospace()
                .size(10.5)
                .color(theme::IVORY),
            );
        });
        let duration = self.doc.animation.effective_duration();
        let mut time = self.doc.animation.time;
        if ui
            .add(egui::Slider::new(&mut time, 0.0..=duration).text("Head"))
            .changed()
        {
            self.set_preview_time(time);
        }

        let mut animation_changed = false;
        animation_changed |= ui
            .add(
                egui::Slider::new(&mut self.doc.animation.duration, 0.5..=120.0)
                    .text("Timeline")
                    .suffix(" s"),
            )
            .changed();
        ui.horizontal(|ui| {
            ui.label(RichText::new("FPS").size(11.0).color(theme::MUTED));
            animation_changed |= ui
                .add(egui::DragValue::new(&mut self.doc.animation.fps).range(1..=120))
                .changed();
            ui.label(RichText::new("Speed").size(11.0).color(theme::MUTED));
            animation_changed |= ui
                .add(
                    egui::DragValue::new(&mut self.doc.animation.speed)
                        .speed(0.02)
                        .range(0.05..=8.0),
                )
                .changed();
        });
        animation_changed |= ui
            .checkbox(&mut self.doc.animation.loop_playback, "Loop")
            .changed();
        animation_changed |= ui
            .add(
                egui::Slider::new(&mut self.doc.animation.transition_seconds, 0.0..=8.0)
                    .text("Shot blend")
                    .suffix(" s"),
            )
            .changed();
        if animation_changed {
            self.note_ui_edit("Playback settings");
        }

        theme::divider(ui);
        theme::section_label(ui, "Turntable");
        let mut turntable_changed = false;
        turntable_changed |= ui
            .checkbox(&mut self.doc.animation.turntable, "Turntable camera")
            .changed();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Axis").size(11.0).color(theme::MUTED));
            for axis in [TurntableAxis::Y, TurntableAxis::X, TurntableAxis::Z] {
                if ui
                    .selectable_label(self.doc.animation.turntable_axis == axis, axis.label())
                    .clicked()
                {
                    self.doc.animation.turntable_axis = axis;
                    turntable_changed = true;
                }
            }
        });
        turntable_changed |= ui
            .add(
                egui::Slider::new(&mut self.doc.animation.turntable_turns, 0.1..=8.0).text("Turns"),
            )
            .changed();
        turntable_changed |= ui
            .add(
                egui::Slider::new(&mut self.doc.animation.motion_blur, 0.0..=1.0)
                    .text("Motion blur"),
            )
            .changed();
        if turntable_changed {
            self.note_ui_edit("Turntable");
        }

        theme::divider(ui);
        theme::section_label(ui, "Camera shots");
        #[derive(Clone)]
        struct ShotRow {
            index: usize,
            name: String,
            duration: f32,
            easing: Easing,
        }
        let shots: Vec<ShotRow> = self
            .doc
            .animation
            .shots
            .iter()
            .enumerate()
            .map(|(index, shot)| ShotRow {
                index,
                name: shot.name.clone(),
                duration: shot.duration,
                easing: shot.easing,
            })
            .collect();
        if shots.is_empty() {
            ui.label(
                RichText::new("No shots yet. Compose the camera, then add a shot.")
                    .size(10.5)
                    .color(theme::MUTED),
            );
        }
        let mut apply_shot = None;
        let mut delete_shot = None;
        for row in &shots {
            Frame::new()
                .fill(theme::PANEL_ALT)
                .inner_margin(egui::Margin::symmetric(7, 4))
                .corner_radius(CornerRadius::same(5))
                .stroke(Stroke::new(1.0, theme::LINE_SOFT))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if icon_button(ui, theme::Icon::Camera, false, "Apply this shot").clicked()
                        {
                            apply_shot = Some(row.index);
                        }
                        ui.label(
                            RichText::new(&row.name)
                                .size(10.5)
                                .strong()
                                .color(theme::IVORY),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if icon_button(ui, theme::Icon::Trash, false, "Remove shot").clicked() {
                                delete_shot = Some(row.index);
                            }
                            ui.label(
                                RichText::new(format!("{:.1}s", row.duration))
                                    .size(9.5)
                                    .color(theme::FAINT),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Hold").size(9.5).color(theme::FAINT));
                        let mut duration = row.duration;
                        if ui
                            .add(
                                egui::DragValue::new(&mut duration)
                                    .speed(0.05)
                                    .range(0.1..=120.0)
                                    .suffix(" s"),
                            )
                            .changed()
                        {
                            if let Some(shot) = self.doc.animation.shots.get_mut(row.index) {
                                shot.duration = duration;
                            }
                            self.note_ui_edit("Shot duration");
                        }
                        let mut easing = row.easing;
                        egui::ComboBox::from_id_salt(("shot-easing", row.index))
                            .selected_text(easing.label())
                            .width(96.0)
                            .show_ui(ui, |ui| {
                                for option in [
                                    Easing::Linear,
                                    Easing::Smooth,
                                    Easing::EaseIn,
                                    Easing::EaseOut,
                                    Easing::EaseInOut,
                                ] {
                                    if ui
                                        .selectable_value(&mut easing, option, option.label())
                                        .clicked()
                                    {
                                        if let Some(shot) =
                                            self.doc.animation.shots.get_mut(row.index)
                                        {
                                            shot.easing = option;
                                        }
                                        self.note_ui_edit("Shot easing");
                                    }
                                }
                            });
                    });
                });
            ui.add_space(2.0);
        }
        if let Some(index) = apply_shot {
            self.exit_preview();
            self.doc.apply_camera_shot(index);
            self.needs_render = true;
            self.status = "Applied camera shot".into();
        }
        if let Some(index) = delete_shot {
            self.record("Remove camera shot");
            self.doc.animation.shots.remove(index);
            self.touch_document();
        }
        if ui.button("Add current camera as shot").clicked() {
            self.record("Add camera shot");
            let name = format!("Shot {}", self.doc.animation.shots.len() + 1);
            self.doc.add_camera_shot(name);
            self.touch_document();
        }

        theme::divider(ui);
        theme::section_label(ui, "Camera keyframes");
        let keyframes: Vec<(usize, f32, Easing)> = self
            .doc
            .animation
            .keyframes
            .iter()
            .enumerate()
            .map(|(index, key)| (index, key.time, key.easing))
            .collect();
        if keyframes.is_empty() {
            ui.label(
                RichText::new("Keyframes blend the editor camera through a timed path.")
                    .size(10.5)
                    .color(theme::MUTED),
            );
        }
        let mut seek_key = None;
        let mut delete_key = None;
        for (index, time, easing) in keyframes {
            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new(
                        RichText::new(format!("{time:.2}s")).monospace().size(10.0),
                    ))
                    .on_hover_text("Seek to keyframe")
                    .clicked()
                {
                    seek_key = Some(time);
                }
                let mut edit_time = time;
                if ui
                    .add(
                        egui::DragValue::new(&mut edit_time)
                            .speed(0.02)
                            .range(0.0..=120.0)
                            .suffix(" s"),
                    )
                    .changed()
                {
                    if let Some(key) = self.doc.animation.keyframes.get_mut(index) {
                        key.time = edit_time;
                    }
                    self.doc
                        .animation
                        .keyframes
                        .sort_by(|a, b| a.time.total_cmp(&b.time));
                    self.note_ui_edit("Keyframe time");
                }
                let mut easing_value = easing;
                egui::ComboBox::from_id_salt(("key-easing", index))
                    .selected_text(easing_value.label())
                    .width(92.0)
                    .show_ui(ui, |ui| {
                        for option in [
                            Easing::Linear,
                            Easing::Smooth,
                            Easing::EaseIn,
                            Easing::EaseOut,
                            Easing::EaseInOut,
                        ] {
                            if ui
                                .selectable_value(&mut easing_value, option, option.label())
                                .clicked()
                            {
                                if let Some(key) = self.doc.animation.keyframes.get_mut(index) {
                                    key.easing = option;
                                }
                                self.note_ui_edit("Keyframe easing");
                            }
                        }
                    });
                if icon_button(ui, theme::Icon::Trash, false, "Remove keyframe").clicked() {
                    delete_key = Some(index);
                }
            });
        }
        if let Some(time) = seek_key {
            self.set_preview_time(time);
        }
        if let Some(index) = delete_key {
            self.record("Remove keyframe");
            self.doc.animation.keyframes.remove(index);
            self.touch_document();
        }
        ui.horizontal(|ui| {
            if ui.button("Key current camera").clicked() {
                self.record("Add camera keyframe");
                let time = self.doc.animation.time;
                let camera = self.doc.camera.clone();
                self.doc.animation.add_keyframe(time, &camera);
                self.touch_document();
            }
            if ui.button("Clear path").clicked() {
                self.record("Clear camera path");
                self.doc.animation.keyframes.clear();
                self.touch_document();
            }
        });
        ui.label(
            RichText::new(format!(
                "{} frames at {} fps · {:.1} MB estimated capture",
                (self.doc.animation.effective_duration() * self.doc.animation.fps as f32).ceil(),
                self.doc.animation.fps,
                (self.export.width as f32
                    * self.export.height as f32
                    * 4.0
                    * self.doc.animation.effective_duration()
                    * self.doc.animation.fps as f32)
                    / 1_048_576.0
            ))
            .size(9.5)
            .color(theme::FAINT),
        );
    }
}

// === VIEWPORT ===

impl PlumeApp {
    fn viewport(&mut self, ui: &mut egui::Ui) {
        let rect = ui.available_rect_before_wrap();
        let response = self.viewport_response(ui, rect);
        self.handle_viewport_input(ui, rect, &response);

        let ctx = ui.ctx().clone();
        let texture = self.render_view(&ctx, rect);
        let camera = self.view_camera();
        let size = Vec2::new(rect.width().max(1.0), rect.height().max(1.0));
        let painter = ui.painter_at(rect);

        painter.rect_filled(rect, CornerRadius::ZERO, theme::VOID);
        if let Some(texture) = texture {
            painter.image(
                texture,
                rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "PLUME could not initialise a GPU renderer.\nThe editor is still usable for export and document work.",
                FontId::new(13.0, egui::FontFamily::Proportional),
                theme::MUTED,
            );
        }

        self.paint_gizmo(&painter, rect, &camera, size);
        self.paint_marquee(&painter, rect);
        self.paint_brush_cursor(ui, &painter, rect, &camera, size);

        if self.show_stats {
            self.paint_viewport_stats(&painter, rect, &camera);
        }

        // Top viewport strip: snaps on the left, display toggles on the right.
        let hud = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 40.0));
        painter.rect_filled(
            hud,
            CornerRadius::ZERO,
            theme::INK_SOFT.gamma_multiply(0.92),
        );
        painter.hline(
            hud.x_range(),
            hud.max.y - 0.5,
            Stroke::new(1.0, theme::LINE_SOFT),
        );
        let hud_inner = hud.shrink2(Vec2::new(10.0, 6.0));
        ui.scope_builder(UiBuilder::new().max_rect(hud_inner), |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (snap, short) in [
                    (ViewSnap::Perspective, "Persp"),
                    (ViewSnap::Top, "Top"),
                    (ViewSnap::Front, "Front"),
                    (ViewSnap::Right, "Right"),
                ] {
                    let response = ui.add(
                        egui::Button::new(RichText::new(short).size(10.0).color(theme::MUTED))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE)
                            .corner_radius(CornerRadius::same(4))
                            .min_size(Vec2::new(42.0, 22.0)),
                    );
                    if response.clicked() {
                        self.snap_view(snap);
                    }
                }
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                let guide_name = self
                    .doc
                    .scene
                    .active_guide
                    .and_then(|id| self.doc.scene.guide(id))
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "Camera plane".to_string());
                ui.label(
                    RichText::new(format!("Drawing on · {guide_name}"))
                        .size(10.0)
                        .color(theme::TEAL),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let stats = self.doc.stats();
                    ui.label(
                        RichText::new(format!(
                            "{} curves  ·  {} guides  ·  {:.0} mm",
                            stats.curves,
                            stats.guides,
                            camera.distance()
                        ))
                        .size(9.5)
                        .color(theme::FAINT),
                    );
                    ui.add_space(6.0);
                    if ui
                        .selectable_label(self.doc.render.grid, "Grid")
                        .on_hover_text("Ground grid")
                        .clicked()
                    {
                        self.doc.render.grid = !self.doc.render.grid;
                        self.needs_render = true;
                    }
                    if ui
                        .selectable_label(self.show_guides, "Guides")
                        .on_hover_text("Construction surfaces")
                        .clicked()
                    {
                        self.show_guides = !self.show_guides;
                        self.needs_render = true;
                    }
                    egui::ComboBox::from_id_salt("hud-quality")
                        .selected_text(self.quality.label())
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for quality in [
                                MeshQuality::Draft,
                                MeshQuality::Normal,
                                MeshQuality::High,
                                MeshQuality::Export,
                            ] {
                                if ui
                                    .selectable_value(&mut self.quality, quality, quality.label())
                                    .clicked()
                                {
                                    self.needs_render = true;
                                }
                            }
                        });
                });
            });
        });

        if self.doc.scene.curves.is_empty() {
            let prompt = Rect::from_center_size(
                rect.center() + Vec2::new(0.0, 36.0),
                Vec2::new(410.0, 72.0),
            );
            painter.rect(
                prompt,
                CornerRadius::same(9),
                theme::INK.gamma_multiply(0.82),
                Stroke::new(1.0, theme::LINE_SOFT),
                StrokeKind::Inside,
            );
            painter.text(
                prompt.center() - Vec2::new(0.0, 12.0),
                Align2::CENTER_CENTER,
                "Draw your first stroke",
                FontId::new(14.0, egui::FontFamily::Proportional),
                theme::IVORY,
            );
            painter.text(
                prompt.center() + Vec2::new(0.0, 10.0),
                Align2::CENTER_CENTER,
                "Pick a brush on the left, then drag across the ground plane. Right-drag orbits.",
                FontId::new(10.5, egui::FontFamily::Proportional),
                theme::MUTED,
            );
        }
    }

    fn paint_gizmo(
        &mut self,
        painter: &egui::Painter,
        rect: Rect,
        camera: &plume_core::viewport::Camera,
        size: Vec2,
    ) {
        if !matches!(self.tool, Tool::Move) {
            return;
        }
        let Some(centre) = self.doc.scene.selection_centre() else {
            return;
        };
        let handles = transform::gizmo_handles(
            centre,
            camera,
            plume_core::Vec2::new(size.x, size.y),
            self.settings.transform_mode,
        );
        for handle in handles {
            let color = axis_color(handle.axis);
            let a = Pos2::new(rect.left() + handle.start.x, rect.top() + handle.start.y);
            let b = Pos2::new(rect.left() + handle.end.x, rect.top() + handle.end.y);
            painter.line_segment([a, b], Stroke::new(1.8, color));
            painter.circle_filled(b, 4.2, color);
            painter.text(
                b + (b - a).normalized() * 9.0,
                Align2::CENTER_CENTER,
                handle.axis.label(),
                FontId::new(9.5, egui::FontFamily::Proportional),
                color,
            );
        }
    }

    fn paint_marquee(&self, painter: &egui::Painter, rect: Rect) {
        if let crate::app::Drag::Marquee { start, current, .. } = &self.drag {
            let a = Pos2::new(rect.left() + start.x, rect.top() + start.y);
            let b = Pos2::new(rect.left() + current.x, rect.top() + current.y);
            let marquee = Rect::from_two_pos(a, b);
            painter.rect(
                marquee,
                CornerRadius::same(2),
                theme::TEAL.gamma_multiply(0.12),
                Stroke::new(1.0, theme::TEAL),
                StrokeKind::Inside,
            );
        }
    }

    fn paint_brush_cursor(
        &self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        camera: &plume_core::viewport::Camera,
        size: Vec2,
    ) {
        let Some(pointer) = ui.ctx().input(|i| i.pointer.hover_pos()) else {
            return;
        };
        if !rect.contains(pointer) || pointer.y < rect.top() + 40.0 {
            return;
        }
        let local = Vec2::new(pointer.x - rect.left(), pointer.y - rect.top());
        let color = match self.tool {
            Tool::Brush => theme::IVORY,
            Tool::Erase => theme::RED,
            Tool::Liquify => theme::LILAC,
            _ => return,
        };
        let radius = match self.tool {
            Tool::Liquify => self.settings.liquify.radius_px.clamp(2.0, 400.0),
            _ => {
                let scale = crate::gpu::world_to_screen_scale(
                    camera,
                    camera.target,
                    plume_core::Vec2::new(size.x, size.y),
                );
                (self.settings.size * 0.5 * scale).clamp(2.0, 360.0)
            }
        };
        painter.circle_stroke(pointer, radius, Stroke::new(1.3, color.gamma_multiply(0.9)));
        painter.circle_filled(pointer, 1.6, color);
        let _ = local;
    }

    fn paint_viewport_stats(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        camera: &plume_core::viewport::Camera,
    ) {
        let area = Rect::from_min_size(rect.min + Vec2::new(12.0, 52.0), Vec2::new(196.0, 78.0));
        painter.rect(
            area,
            CornerRadius::same(7),
            theme::INK.gamma_multiply(0.72),
            Stroke::new(1.0, theme::LINE_SOFT.gamma_multiply(0.8)),
            StrokeKind::Inside,
        );
        let stats = self.doc.stats();
        let lines = [
            format!("{} curves   {} guides", stats.curves, stats.guides),
            format!("{} groups   {} points", stats.groups, stats.control_points),
            format!(
                "cam {:.0}, {:.0}, {:.0}",
                camera.position.x, camera.position.y, camera.position.z
            ),
            format!(
                "{} × {}   {}",
                self.last_render_size[0],
                self.last_render_size[1],
                self.quality.label()
            ),
        ];
        for (index, line) in lines.iter().enumerate() {
            painter.text(
                area.min + Vec2::new(9.0, 9.0 + index as f32 * 16.0),
                Align2::LEFT_TOP,
                line,
                FontId::new(9.5, egui::FontFamily::Monospace),
                if index == 0 {
                    theme::IVORY
                } else {
                    theme::FAINT
                },
            );
        }
    }
}

fn axis_color(axis: transform::Axis) -> Color32 {
    match axis {
        transform::Axis::X => theme::CORAL,
        transform::Axis::Y => theme::GREEN,
        transform::Axis::Z => theme::BLUE,
    }
}

// === TIMELINE ===

impl PlumeApp {
    fn timeline(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if icon_button(ui, theme::Icon::Rewind, false, "Return to start").clicked() {
                self.stop_playback();
            }
            if icon_button(
                ui,
                if self.doc.animation.playing {
                    theme::Icon::Pause
                } else {
                    theme::Icon::Play
                },
                self.doc.animation.playing,
                "Play or pause (Space)",
            )
            .clicked()
            {
                self.toggle_playback();
            }
            if icon_button(ui, theme::Icon::Stop, false, "Stop").clicked() {
                self.stop_playback();
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{:0>5.2}", self.doc.animation.time))
                    .monospace()
                    .size(11.0)
                    .color(theme::IVORY),
            );
            ui.label(
                RichText::new(format!(
                    "/ {:.2} s",
                    self.doc.animation.effective_duration()
                ))
                .size(9.5)
                .color(theme::FAINT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(
                    ui,
                    theme::Icon::Camera,
                    false,
                    "Add a camera shot at the playhead",
                )
                .clicked()
                {
                    self.record("Add camera shot");
                    let name = format!("Shot {}", self.doc.animation.shots.len() + 1);
                    self.doc.add_camera_shot(name);
                    self.touch_document();
                }
                if icon_button(
                    ui,
                    theme::Icon::Key,
                    false,
                    "Key the camera at the playhead",
                )
                .clicked()
                {
                    self.record("Add camera keyframe");
                    let time = self.doc.animation.time;
                    let camera = self.doc.camera.clone();
                    self.doc.animation.add_keyframe(time, &camera);
                    self.touch_document();
                }
                ui.checkbox(&mut self.doc.animation.loop_playback, "Loop");
                ui.label(
                    RichText::new(format!("{} fps", self.doc.animation.fps))
                        .size(9.5)
                        .color(theme::FAINT),
                );
            });
        });

        ui.add_space(3.0);
        let duration = self.doc.animation.effective_duration().max(0.1);
        let (rect, response) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 42.0),
            Sense::click_and_drag(),
        );
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            CornerRadius::same(6),
            theme::INK_SOFT,
            Stroke::new(1.0, theme::LINE_SOFT),
            StrokeKind::Inside,
        );

        let lane = Rect::from_min_max(
            Pos2::new(rect.left() + 6.0, rect.top() + 14.0),
            Pos2::new(rect.right() - 6.0, rect.bottom() - 6.0),
        );

        // Ruler.
        let step = if duration < 8.0 {
            1.0
        } else if duration < 24.0 {
            2.0
        } else {
            5.0
        };
        let mut tick = 0.0;
        while tick <= duration + 0.001 {
            let x = lane.left() + lane.width() * (tick / duration).clamp(0.0, 1.0);
            let major = (tick % (step * 5.0)).abs() < 0.001;
            painter.line_segment(
                [Pos2::new(x, lane.top() - 4.0), Pos2::new(x, lane.bottom())],
                Stroke::new(1.0, theme::LINE_SOFT.gamma_multiply(0.8)),
            );
            if major && x < lane.right() - 20.0 {
                painter.text(
                    Pos2::new(x + 3.0, rect.top() + 4.0),
                    Align2::LEFT_TOP,
                    format!("{tick:.0}s"),
                    FontId::new(8.5, egui::FontFamily::Monospace),
                    theme::FAINT,
                );
            }
            tick += step;
        }

        // Shot bands.
        let shots: Vec<(usize, f32, f32, String)> = self
            .doc
            .animation
            .shots
            .iter()
            .enumerate()
            .map(|(index, shot)| {
                (
                    index,
                    self.doc.animation.shot_start(index),
                    shot.duration.max(0.1),
                    shot.name.clone(),
                )
            })
            .collect();
        for (index, start, length, name) in &shots {
            let x0 = lane.left() + lane.width() * (start / duration).clamp(0.0, 1.0);
            let x1 = lane.left() + lane.width() * ((start + length) / duration).clamp(0.0, 1.0);
            let band = Rect::from_min_max(
                Pos2::new(x0 + 1.0, lane.top() + 5.0),
                Pos2::new((x1 - 1.0).max(x0 + 2.0), lane.bottom() - 3.0),
            );
            let tint = if index % 2 == 0 {
                theme::TEAL
            } else {
                theme::BLUE
            };
            painter.rect(
                band,
                CornerRadius::same(3),
                tint.gamma_multiply(0.20),
                Stroke::new(1.0, tint.gamma_multiply(0.55)),
                StrokeKind::Inside,
            );
            if band.width() > 36.0 {
                painter.text(
                    band.left_top() + Vec2::new(4.0, 3.0),
                    Align2::LEFT_TOP,
                    name,
                    FontId::new(8.5, egui::FontFamily::Proportional),
                    theme::IVORY.gamma_multiply(0.78),
                );
            }
        }

        // Keyframes.
        for key in &self.doc.animation.keyframes {
            let x = lane.left() + lane.width() * (key.time / duration).clamp(0.0, 1.0);
            let y = lane.bottom() - 3.0;
            let points = vec![
                Pos2::new(x, y - 6.0),
                Pos2::new(x + 4.0, y),
                Pos2::new(x, y + 1.0),
                Pos2::new(x - 4.0, y),
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                theme::GOLD,
                Stroke::new(1.0, theme::INK),
            ));
        }

        // Playhead.
        let head =
            lane.left() + lane.width() * (self.doc.animation.time / duration).clamp(0.0, 1.0);
        painter.line_segment(
            [
                Pos2::new(head, rect.top() + 2.0),
                Pos2::new(head, rect.bottom() - 2.0),
            ],
            Stroke::new(1.4, theme::CORAL),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(head - 5.0, rect.top() + 2.0),
                Pos2::new(head + 5.0, rect.top() + 2.0),
                Pos2::new(head, rect.top() + 8.0),
            ],
            theme::CORAL,
            Stroke::NONE,
        ));

        if response.clicked() || response.dragged() {
            if let Some(pointer) = response.interact_pointer_pos() {
                if lane.x_range().contains(pointer.x) {
                    self.doc.animation.pause();
                    let t = ((pointer.x - lane.left()) / lane.width().max(1.0)) * duration;
                    self.set_preview_time(t);
                    self.timeline_drag = true;
                }
            }
        }
        if response.drag_stopped() || response.clicked() {
            self.timeline_drag = false;
        }
        if shots.is_empty() && self.doc.animation.keyframes.is_empty() {
            painter.text(
                lane.center(),
                Align2::CENTER_CENTER,
                "Timeline is empty. Add a camera shot or keyframe from the Motion inspector.",
                FontId::new(9.5, egui::FontFamily::Proportional),
                theme::FAINT,
            );
        }
    }
}

// === DIALOGS ===

impl PlumeApp {
    fn dialogs(&mut self, ctx: &egui::Context) {
        match self.dialog {
            Dialog::Gallery => self.gallery_dialog(ctx),
            Dialog::Export => self.export_dialog(ctx),
            Dialog::Help => self.help_dialog(ctx),
            Dialog::About => self.about_dialog(ctx),
            Dialog::Import => self.import_dialog(ctx),
            Dialog::Recovery => self.recovery_dialog(ctx),
            Dialog::None => {}
        }
        self.confirm_dialog(ctx);
    }

    fn gallery_dialog(&mut self, ctx: &egui::Context) {
        self.ensure_gallery_thumbnails(ctx);
        self.ensure_gallery_preview(ctx);

        let items = self.gallery_items.clone();
        let thumbs = self.gallery_thumbs.clone();
        let preview = self.gallery_preview.clone();
        let mut selected = self.gallery_selected.clone();
        let mut load = None;
        let mut close = false;

        egui::Window::new("Scene gallery")
            .id(Id::unique("plume.gallery"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(900.0)
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.set_width(860.0);
                ui.horizontal(|ui| {
                    theme::heading(
                        ui,
                        "Start from a scene",
                        Some("Every gallery scene is generated by PLUME's own brushes and curves."),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, theme::Icon::Close, false, "Close gallery").clicked() {
                            close = true;
                        }
                    });
                });
                theme::divider(ui);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(428.0);
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::new(428.0, 236.0),
                            Sense::hover(),
                        );
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(8),
                            theme::INK_SOFT,
                            Stroke::new(1.0, theme::LINE_SOFT),
                            StrokeKind::Inside,
                        );
                        if let Some(texture) = &preview {
                            ui.painter().image(
                                texture.id(),
                                rect,
                                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                Color32::WHITE,
                            );
                        } else {
                            theme::thumbnail_plate(ui.painter(), rect);
                        }
                        if let Some(item) = items.iter().find(|item| item.id == selected) {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                theme::pill(ui, item.category, theme::TEAL);
                                ui.label(
                                    RichText::new(item.name)
                                        .size(18.0)
                                        .strong()
                                        .color(theme::IVORY),
                                );
                            });
                            ui.label(
                                RichText::new(item.description).size(11.0).color(theme::MUTED),
                            );
                        }

                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Load scene").strong().color(theme::INK),
                                    )
                                    .fill(theme::CORAL)
                                    .stroke(Stroke::NONE)
                                    .min_size(Vec2::new(130.0, 30.0)),
                                )
                                .clicked()
                            {
                                load = Some(selected.clone());
                            }
                            if ui.button("Cancel").clicked() {
                                close = true;
                            }
                        });
                        ui.label(
                            RichText::new(
                                "The current scene is only replaced after your unsaved changes are handled.",
                            )
                            .size(9.5)
                            .color(theme::FAINT),
                        );
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(8.0);

                    ui.vertical(|ui| {
                        ui.set_width(384.0);
                        egui::ScrollArea::vertical()
                            .id_salt("gallery-list")
                            .max_height(360.0)
                            .show(ui, |ui| {
                                for (index, item) in items.iter().enumerate() {
                                    let chosen = item.id == selected;
                                    let (rect, response) = ui.allocate_exact_size(
                                        Vec2::new(ui.available_width(), 62.0),
                                        Sense::click(),
                                    );
                                    let painter = ui.painter().with_clip_rect(rect);
                                    let fill = if chosen {
                                        theme::CORAL.gamma_multiply(0.16)
                                    } else if response.hovered() {
                                        theme::PANEL_HIGH
                                    } else {
                                        theme::PANEL_ALT
                                    };
                                    painter.rect(
                                        rect,
                                        CornerRadius::same(6),
                                        fill,
                                        Stroke::new(
                                            1.0,
                                            if chosen { theme::CORAL } else { theme::LINE_SOFT },
                                        ),
                                        StrokeKind::Inside,
                                    );
                                    let thumb_rect = Rect::from_min_size(
                                        rect.min + Vec2::new(5.0, 5.0),
                                        Vec2::new(80.0, 52.0),
                                    );
                                    if let Some(Some(texture)) = thumbs.get(index) {
                                        painter.image(
                                            texture.id(),
                                            thumb_rect,
                                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                    } else {
                                        theme::thumbnail_plate(&painter, thumb_rect);
                                    }
                                    painter.text(
                                        thumb_rect.right_top() + Vec2::new(9.0, 2.0),
                                        Align2::LEFT_TOP,
                                        item.name,
                                        FontId::new(11.5, egui::FontFamily::Proportional),
                                        theme::IVORY,
                                    );
                                    painter.text(
                                        thumb_rect.right_top() + Vec2::new(9.0, 20.0),
                                        Align2::LEFT_TOP,
                                        item.description,
                                        FontId::new(9.0, egui::FontFamily::Proportional),
                                        theme::FAINT,
                                    );
                                    painter.text(
                                        thumb_rect.right_bottom() + Vec2::new(9.0, -1.0),
                                        Align2::LEFT_BOTTOM,
                                        item.category,
                                        FontId::new(8.5, egui::FontFamily::Proportional),
                                        theme::TEAL.gamma_multiply(0.8),
                                    );
                                    if response.clicked() {
                                        selected = item.id.to_string();
                                    }
                                    if response.double_clicked() {
                                        load = Some(item.id.to_string());
                                    }
                                    ui.add_space(3.0);
                                }
                            });
                    });
                });
            });

        if selected != self.gallery_selected {
            self.gallery_selected = selected;
            self.refresh_gallery_preview(ctx);
        }
        if let Some(id) = load {
            self.request_gallery(&id);
        } else if close {
            self.dialog = Dialog::None;
        }
    }

    fn export_dialog(&mut self, ctx: &egui::Context) {
        let mut start = false;
        let mut browse = false;
        let mut close = false;
        let mut format_changed = false;

        egui::Window::new("Export")
            .id(Id::unique("plume.export"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(570.0)
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    theme::heading(
                        ui,
                        "Export your work",
                        Some("Models leave PLUME in millimetres and convert on the way out."),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, theme::Icon::Close, false, "Close export").clicked() {
                            close = true;
                        }
                    });
                });
                theme::divider(ui);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Format").size(11.0).color(theme::MUTED));
                    let before = self.export.format;
                    egui::ComboBox::from_id_salt("export-format")
                        .selected_text(self.export.format.label())
                        .width(290.0)
                        .show_ui(ui, |ui| {
                            for format in ExportFormat::ALL {
                                ui.selectable_value(
                                    &mut self.export.format,
                                    format,
                                    format.label(),
                                );
                            }
                        });
                    format_changed = before != self.export.format;
                    theme::pill(
                        ui,
                        self.export.format.extension().to_uppercase(),
                        theme::TEAL,
                    );
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Destination").size(11.0).color(theme::MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.export.path)
                            .desired_width(352.0)
                            .hint_text("Choose a destination inside the PLUME sandbox"),
                    );
                    if ui.button("Browse…").clicked() {
                        browse = true;
                    }
                });
                ui.label(
                    RichText::new(format!(
                        "Sandbox · {}",
                        crate::sandbox::pretty_path(self.sandbox.root())
                    ))
                        .size(9.0)
                        .color(theme::FAINT),
                );

                theme::divider(ui);
                if self.export.format.is_document() {
                    ui.label(
                        RichText::new(
                            "Documents keep curves, guides, materials, lighting, shots and the full render state. A thumbnail is embedded for the gallery.",
                        )
                        .size(11.0)
                        .color(theme::MUTED),
                    );
                } else if self.export.format.is_model() {
                    theme::section_label(ui, "Mesh options");
                    egui::ComboBox::from_id_salt("export-quality")
                        .selected_text(self.export.quality.label())
                        .show_ui(ui, |ui| {
                            for quality in [
                                MeshQuality::Draft,
                                MeshQuality::Normal,
                                MeshQuality::High,
                                MeshQuality::Export,
                            ] {
                                ui.selectable_value(
                                    &mut self.export.quality,
                                    quality,
                                    quality.label(),
                                );
                            }
                        });
                    ui.checkbox(&mut self.export.include_guides, "Include guide meshes");
                    ui.checkbox(&mut self.export.include_hidden, "Include hidden strokes");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut self.export.merge, "Merge by material");
                        ui.checkbox(&mut self.export.z_up, "Z-up coordinates");
                    });
                    ui.add(
                        egui::Slider::new(&mut self.export.unit_scale, 0.0001..=1.0)
                            .text("Unit scale")
                            .logarithmic(true),
                    );
                    ui.label(
                        RichText::new(format!(
                            "1 mm becomes {:.4} output units ({}).",
                            self.export.unit_scale,
                            if self.export.unit_scale == 0.001 {
                                "metres, glTF/USDZ convention"
                            } else if self.export.unit_scale == 1.0 {
                                "millimetres, OBJ/STL convention"
                            } else {
                                "custom"
                            }
                        ))
                        .size(9.5)
                        .color(theme::FAINT),
                    );
                    if self.export.format == ExportFormat::Stl {
                        ui.checkbox(&mut self.export.binary_stl, "Binary STL");
                    }
                } else {
                    theme::section_label(ui, "Image options");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Size").size(11.0).color(theme::MUTED));
                        ui.add(
                            egui::DragValue::new(&mut self.export.width)
                                .range(64..=7680)
                                .suffix(" px"),
                        );
                        ui.label(RichText::new("×").color(theme::FAINT));
                        ui.add(
                            egui::DragValue::new(&mut self.export.height)
                                .range(64..=7680)
                                .suffix(" px"),
                        );
                        egui::ComboBox::from_id_salt("export-quality-media")
                            .selected_text(self.export.quality.label())
                            .width(88.0)
                            .show_ui(ui, |ui| {
                                for quality in [
                                    MeshQuality::Draft,
                                    MeshQuality::Normal,
                                    MeshQuality::High,
                                    MeshQuality::Export,
                                ] {
                                    ui.selectable_value(
                                        &mut self.export.quality,
                                        quality,
                                        quality.label(),
                                    );
                                }
                            });
                    });
                    if self.export.format == ExportFormat::Png {
                        ui.checkbox(
                            &mut self.export.transparent,
                            "Transparent background (renderer dependent)",
                        );
                    } else {
                        ui.add(
                            egui::Slider::new(&mut self.export.duration, 0.5..=120.0)
                                .text("Duration")
                                .suffix(" s"),
                        );
                        let fps_changed = self.export.format == ExportFormat::Gif;
                        if fps_changed {
                            ui.add(
                                egui::Slider::new(&mut self.export.gif_fps, 2..=30)
                                    .text("GIF frame rate"),
                            );
                        } else {
                            ui.add(
                                egui::Slider::new(&mut self.export.fps, 1..=60).text("Frame rate"),
                            );
                        }
                        let fps = if fps_changed {
                            self.export.gif_fps
                        } else {
                            self.export.fps
                        };
                        let frames = (self.export.duration * fps as f32).ceil().max(1.0) as u32;
                        ui.label(
                            RichText::new(format!(
                                "{frames} frames at {} × {} px. MP4 export needs ffmpeg on PATH.",
                                self.export.width, self.export.height
                            ))
                            .size(9.5)
                            .color(theme::FAINT),
                        );
                    }
                    if self.media.is_some() {
                        ui.label(
                            RichText::new("A capture is already running. Wait for it to finish.")
                                .size(10.0)
                                .color(theme::GOLD),
                        );
                    }
                }

                theme::divider(ui);
                ui.horizontal(|ui| {
                    let enabled = !self.export.path.trim().is_empty() && self.media.is_none();
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(
                                RichText::new("Export").strong().color(theme::INK),
                            )
                            .fill(theme::CORAL)
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(120.0, 30.0)),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if self.export.format.is_media() {
                                "Renders on a background worker"
                            } else {
                                "Written synchronously"
                            })
                            .size(9.5)
                            .color(theme::FAINT),
                        );
                    });
                });
            });

        if format_changed {
            self.sync_export_path();
        }
        if browse {
            let ext = self.export.format.extension();
            let mut dialog = self
                .file_dialog()
                .set_title("Choose an export destination")
                .add_filter(self.export.format.label(), &[ext]);
            let start = match std::path::Path::new(&self.export.path).parent() {
                Some(parent) if parent.exists() => parent.to_path_buf(),
                _ => self.sandbox.projects_dir_ready(),
            };
            dialog = dialog.set_directory(start);
            if let Some(path) = dialog.save_file() {
                if self.sandbox.contains(&path) {
                    self.export.path = crate::sandbox::pretty_path(&path);
                } else {
                    self.notify("Choose a destination inside the PLUME sandbox.", true);
                }
            }
        }
        if start {
            self.run_export();
        }
        if close {
            self.dialog = Dialog::None;
        }
    }

    fn help_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::Window::new("Quick start")
            .id(Id::unique("plume.help"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(660.0)
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    theme::heading(
                        ui,
                        "Make a line, give it dimension",
                        Some("PLUME is a 3D curve studio. Everything starts with a stroke."),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, theme::Icon::Close, false, "Close help").clicked() {
                            close = true;
                        }
                    });
                });
                theme::divider(ui);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(310.0);
                        theme::section_label(ui, "Workflow");
                        for (step, text) in [
                            ("1", "Choose a brush in the left drawer and drag across the ground plane."),
                            ("2", "Add a guide — plane, sphere, box, cylinder, cone or torus — to draw in space."),
                            ("3", "Use Select and Move to shape the composition; mirror or group strokes."),
                            ("4", "Open Motion to place camera shots and keyframes."),
                            ("5", "Export a model, a still, or an animation from the top bar."),
                        ] {
                            ui.horizontal(|ui| {
                                theme::pill(ui, step, theme::CORAL);
                                ui.label(RichText::new(text).size(11.0).color(theme::MUTED));
                            });
                            ui.add_space(4.0);
                        }
                        theme::section_label(ui, "Navigation");
                        for (keys, text) in [
                            ("Right-drag", "Orbit the camera"),
                            ("Shift + right-drag", "Pan"),
                            ("Middle-drag", "Pan"),
                            ("Wheel", "Zoom"),
                            ("Alt + left-drag", "Orbit"),
                            ("F / Home", "Frame selection / frame scene"),
                            ("Shift + Home", "Frame artwork, ignoring guides"),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(keys)
                                        .monospace()
                                        .size(10.0)
                                        .color(theme::TEAL),
                                );
                                ui.label(RichText::new(text).size(10.0).color(theme::MUTED));
                            });
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        theme::section_label(ui, "Shortcuts");
                        for (keys, text) in [
                            ("B / E / L", "Brush, erase, liquify"),
                            ("V / T / G", "Select, move, guide"),
                            ("C", "Straight line (drag)"),
                            ("Space", "Play or pause"),
                            ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                            ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
                            ("Ctrl+O / Ctrl+N", "Open / new"),
                            ("Ctrl+D / Ctrl+G", "Duplicate / group"),
                            ("Ctrl+Shift+G", "Ungroup"),
                            ("Delete", "Delete selection"),
                            ("[ / ]", "Brush size down / up"),
                            ("1 / 2 / 3 / 4", "Perspective / top / front / right"),
                        ] {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(keys)
                                        .monospace()
                                        .size(9.5)
                                        .color(theme::GOLD),
                                );
                                ui.label(RichText::new(text).size(10.0).color(theme::MUTED));
                            });
                        }
                        ui.add_space(8.0);
                        theme::section_label(ui, "Files");
                        ui.label(
                            RichText::new(
                                "PLUME documents use a .plume ZIP package with a JSON manifest and thumbnail. Feather .feather files are imported best-effort and reported honestly.",
                            )
                            .size(10.0)
                            .color(theme::MUTED),
                        );
                    });
                });
            });
        if close {
            self.dialog = Dialog::None;
        }
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        let mut close = false;
        egui::Window::new("About PLUME")
            .id(Id::unique("plume.about"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                theme::wordmark(ui);
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .monospace()
                        .size(10.5)
                        .color(theme::FAINT),
                );
                theme::divider(ui);
                ui.label(
                    RichText::new(
                        "A from-scratch studio for drawing dimensional lines in space. Built in Rust with wgpu, egui and a reusable core that also backs the Android shell.",
                    )
                    .size(11.5)
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "PLUME is an independent implementation. Its code, wordmark, icons, gallery scenes and shaders are original. It does not bundle or reproduce Feather's artwork, fonts, trademarks, binaries or assets.",
                    )
                    .size(10.5)
                    .color(theme::MUTED),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    theme::pill(ui, "Rust", theme::CORAL);
                    theme::pill(ui, "wgpu", theme::TEAL);
                    theme::pill(ui, "UniFFI", theme::GOLD);
                    theme::pill(ui, "ARCore shell", theme::LILAC);
                });
                theme::divider(ui);
                ui.label(
                    RichText::new(
                        "Your files stay inside the sandbox shown in the export dialog. No network calls, telemetry or bundled third-party artwork.",
                    )
                    .size(10.0)
                    .color(theme::FAINT),
                );
                ui.add_space(8.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        if close {
            self.dialog = Dialog::None;
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        let Some(pending) = self.pending_import.clone() else {
            self.dialog = Dialog::None;
            return;
        };
        let mut unit = pending.unit;
        let mut confirm = false;
        let mut cancel = false;
        let file = pending
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("mesh")
            .to_string();
        let title = if pending.replace {
            "Open 3D mesh"
        } else {
            "Import 3D mesh"
        };
        egui::Modal::new(Id::unique("plume.import"))
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.set_width(430.0);
                theme::heading(
                    ui,
                    title,
                    Some("Choose the units this file was authored in."),
                );
                theme::divider(ui);
                ui.label(RichText::new(file).size(12.0).strong().color(theme::IVORY));
                ui.label(
                    RichText::new(crate::sandbox::pretty_path(&pending.path))
                        .size(9.5)
                        .color(theme::FAINT),
                );
                ui.add_space(8.0);
                theme::section_label(ui, "Source units");
                for choice in ImportUnit::ALL {
                    ui.radio_value(&mut unit, choice, choice.label());
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "PLUME stores millimetres. Nothing is inferred from the file: choose the unit you authored it in.",
                    )
                    .size(10.0)
                    .color(theme::MUTED),
                );
                theme::divider(ui);
                ui.horizontal(|ui| {
                    let action = if pending.replace { "Open" } else { "Import" };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(action).strong().color(theme::INK),
                            )
                            .fill(theme::CORAL)
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(108.0, 28.0)),
                        )
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if let Some(pending) = self.pending_import.as_mut() {
            pending.unit = unit;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            cancel = true;
        }
        if confirm {
            self.confirm_import();
        } else if cancel {
            self.pending_import = None;
            self.dialog = Dialog::None;
        }
    }

    fn recovery_dialog(&mut self, ctx: &egui::Context) {
        if !self.recovery_pending {
            self.dialog = Dialog::None;
            return;
        }
        let path = self.autosave_path();
        let mut recover = false;
        let mut discard = false;
        egui::Modal::new(Id::unique("plume.recovery"))
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.set_width(460.0);
                theme::heading(
                    ui,
                    "Recover unsaved work?",
                    Some("PLUME found autosaved work from an earlier session."),
                );
                theme::divider(ui);
                ui.label(
                    RichText::new(
                        "The recovery copy is newer than the last document you saved. Recovering opens it as an unsaved scene; discarding deletes the copy.",
                    )
                    .size(11.0)
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(crate::sandbox::pretty_path(&path))
                        .size(9.5)
                        .color(theme::FAINT),
                );
                theme::divider(ui);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Recover work").strong().color(theme::INK),
                            )
                            .fill(theme::CORAL)
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(124.0, 28.0)),
                        )
                        .clicked()
                    {
                        recover = true;
                    }
                    if ui.button("Discard").clicked() {
                        discard = true;
                    }
                });
            });
        if recover {
            self.recover_autosave();
        } else if discard {
            self.discard_autosave();
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_confirm.clone() else {
            return;
        };
        let mut choice = None;
        egui::Window::new("Unsaved changes")
            .id(Id::unique("plume.confirm"))
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .collapsible(false)
            .resizable(false)
            .default_width(430.0)
            .frame(theme::panel_frame())
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("This scene has changes that are not saved.")
                        .size(12.0)
                        .color(theme::IVORY),
                );
                ui.label(
                    RichText::new(match &action {
                        ConfirmAction::New => "Creating a new scene will replace the current one.",
                        ConfirmAction::Open => {
                            "Opening another scene will replace the current one."
                        }
                        ConfirmAction::Gallery(_) => {
                            "Loading a gallery scene will replace the current one."
                        }
                        ConfirmAction::Quit => "Closing PLUME will discard the current changes.",
                    })
                    .size(11.0)
                    .color(theme::MUTED),
                );
                theme::divider(ui);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Save and continue")
                                    .strong()
                                    .color(theme::INK),
                            )
                            .fill(theme::CORAL)
                            .stroke(Stroke::NONE)
                            .min_size(Vec2::new(140.0, 28.0)),
                        )
                        .clicked()
                    {
                        choice = Some(true);
                    }
                    if ui.button("Discard changes").clicked() {
                        choice = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_confirm = None;
                    }
                });
            });
        if let Some(save) = choice {
            self.pending_confirm = None;
            self.apply_confirm(action, save);
        }
    }

    fn show_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some(toast) = &self.toast {
            if now > toast.until {
                self.toast = None;
                return;
            }
        }
        let Some(toast) = &self.toast else {
            return;
        };
        let color = if toast.error { theme::RED } else { theme::TEAL };
        egui::Area::new(Id::unique("plume.toast"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -34.0))
            .show(ctx, |ui| {
                Frame::new()
                    .fill(theme::INK.gamma_multiply(0.94))
                    .stroke(Stroke::new(1.0, color.gamma_multiply(0.65)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(egui::Margin::symmetric(13, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 3.0, color);
                            ui.label(RichText::new(&toast.message).size(11.0).color(theme::IVORY));
                        });
                    });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{close_request_outcome, CloseRequestOutcome};

    #[test]
    fn clean_close_does_not_prompt() {
        assert_eq!(
            close_request_outcome(false, false, false),
            CloseRequestOutcome::Allow
        );
    }

    #[test]
    fn dirty_close_prompts_once() {
        assert_eq!(
            close_request_outcome(true, false, false),
            CloseRequestOutcome::Prompt
        );
        assert_eq!(
            close_request_outcome(true, true, false),
            CloseRequestOutcome::KeepOpen
        );
    }

    #[test]
    fn approved_close_is_not_intercepted_again() {
        assert_eq!(
            close_request_outcome(true, false, true),
            CloseRequestOutcome::Allow
        );
    }
}
