//! Application state, editing logic and the render-on-demand loop.
//!
//! The desktop harness is deliberately thin: every geometric operation lives
//! in `plume-core`, so the Android shell can reuse the same behaviour.  This
//! module owns the transient editor state (selection tool, undo snapshots,
//! playback preview and background capture jobs) and talks to the wgpu
//! renderer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use eframe::egui;
use egui::{Rect, TextureId, Vec2 as EVec2};
use plume_core::brush::{Brush, BrushLibrary};
use plume_core::capture::{self, StrokeConfig};
use plume_core::gallery::{self, GalleryItem};
use plume_core::guide::{Guide, GuideKind};
use plume_core::io::{self, ExportOptions};
use plume_core::liquify::{self, LiquifyBrush};
use plume_core::scene::{Document, History, MeshQuality};
use plume_core::selection::{self, PickSettings};
use plume_core::shapes;
use plume_core::transform::{self, Axis, SnapMode, TransformMode};
use plume_core::viewport::{Camera, ViewSnap};
use plume_core::{CurveId, Mat4, Vec2, Vec3};

use crate::gpu::{self, GpuRenderer};
use crate::sandbox::{pretty_path, Sandbox};
use crate::theme;

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Brush,
    Erase,
    Liquify,
    Select,
    Move,
    Guide,
    Line,
    Circle,
    Ellipse,
    Polygon,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Brush,
        Tool::Erase,
        Tool::Liquify,
        Tool::Select,
        Tool::Move,
        Tool::Guide,
        Tool::Line,
        Tool::Circle,
        Tool::Ellipse,
        Tool::Polygon,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Brush => "Brush",
            Tool::Erase => "Erase",
            Tool::Liquify => "Liquify",
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Guide => "Guide",
            Tool::Line => "Line",
            Tool::Circle => "Circle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Brush => "Draw a stroke on the active guide (B)",
            Tool::Erase => "Remove strokes under the cursor (E)",
            Tool::Liquify => "Push, pull, twirl or pinch stroke points (L)",
            Tool::Select => "Click or drag a marquee to select (V)",
            Tool::Move => "Transform the selection with the axis gizmo (T)",
            Tool::Guide => "Place a construction plane on a surface (G)",
            Tool::Line => "Drag a straight stroke (C)",
            Tool::Circle => "Drag from the centre to draw a circle",
            Tool::Ellipse => "Drag an ellipse on the drawing plane",
            Tool::Polygon => "Drag a regular polygon",
        }
    }

    pub fn shortcut(self) -> Option<egui::Key> {
        Some(match self {
            Tool::Brush => egui::Key::B,
            Tool::Erase => egui::Key::E,
            Tool::Liquify => egui::Key::L,
            Tool::Select => egui::Key::V,
            Tool::Move => egui::Key::T,
            Tool::Guide => egui::Key::G,
            Tool::Line => egui::Key::C,
            _ => return None,
        })
    }

    pub fn icon(self) -> theme::Icon {
        match self {
            Tool::Brush => theme::Icon::Brush,
            Tool::Erase => theme::Icon::Eraser,
            Tool::Liquify => theme::Icon::Liquify,
            Tool::Select => theme::Icon::Select,
            Tool::Move => theme::Icon::Move,
            Tool::Guide => theme::Icon::Guide,
            Tool::Line => theme::Icon::Line,
            Tool::Circle => theme::Icon::Circle,
            Tool::Ellipse => theme::Icon::Ellipse,
            Tool::Polygon => theme::Icon::Smooth,
        }
    }

    fn is_shape(self) -> bool {
        matches!(
            self,
            Tool::Line | Tool::Circle | Tool::Ellipse | Tool::Polygon
        )
    }
}

// ---------------------------------------------------------------------------
// Left panel tabs
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeftTab {
    Tools,
    Brushes,
    Guides,
    Layers,
}

impl LeftTab {
    pub const ALL: [LeftTab; 4] = [
        LeftTab::Tools,
        LeftTab::Brushes,
        LeftTab::Guides,
        LeftTab::Layers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LeftTab::Tools => "Tools",
            LeftTab::Brushes => "Brushes",
            LeftTab::Guides => "Guides",
            LeftTab::Layers => "Layers",
        }
    }
}

// ---------------------------------------------------------------------------
// Tool settings
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ToolSettings {
    /// Stroke diameter in millimetres.
    pub size: f32,
    /// Uniform pressure used by mouse and trackpad input.
    pub pressure: f32,
    pub smoothing: f32,
    pub spacing: f32,
    pub stable_stroke: bool,
    pub shape_sides: u32,
    pub constrain: bool,
    pub snap: SnapMode,
    pub draw_on_guide: bool,
    pub liquify: LiquifyBrush,
    pub transform_mode: TransformMode,
    pub move_plane: bool,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            size: 8.0,
            pressure: 0.86,
            smoothing: 0.18,
            spacing: 2.0,
            stable_stroke: false,
            shape_sides: 6,
            constrain: false,
            snap: SnapMode::Off,
            draw_on_guide: true,
            liquify: LiquifyBrush::default(),
            transform_mode: TransformMode::Translate,
            move_plane: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Dialogs & transient actions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dialog {
    None,
    Gallery,
    Export,
    Help,
    About,
}

#[derive(Clone, Debug)]
pub enum ConfirmAction {
    New,
    Open,
    Gallery(String),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorTab {
    Style,
    Surface,
    Render,
    Motion,
}

impl InspectorTab {
    pub const ALL: [InspectorTab; 4] = [
        InspectorTab::Style,
        InspectorTab::Surface,
        InspectorTab::Render,
        InspectorTab::Motion,
    ];

    pub fn label(self) -> &'static str {
        match self {
            InspectorTab::Style => "Style",
            InspectorTab::Surface => "Surface",
            InspectorTab::Render => "Render",
            InspectorTab::Motion => "Motion",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Plume,
    Feather,
    Obj,
    Glb,
    Gltf,
    Stl,
    Usdz,
    Png,
    Gif,
    Mp4,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 10] = [
        ExportFormat::Plume,
        ExportFormat::Feather,
        ExportFormat::Obj,
        ExportFormat::Glb,
        ExportFormat::Gltf,
        ExportFormat::Stl,
        ExportFormat::Usdz,
        ExportFormat::Png,
        ExportFormat::Gif,
        ExportFormat::Mp4,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Plume => "PLUME document",
            ExportFormat::Feather => "Feather-compatible document",
            ExportFormat::Obj => "Wavefront OBJ",
            ExportFormat::Glb => "glTF binary (GLB)",
            ExportFormat::Gltf => "glTF 2.0 (embedded)",
            ExportFormat::Stl => "STL mesh",
            ExportFormat::Usdz => "USDZ (AR Quick Look)",
            ExportFormat::Png => "PNG still",
            ExportFormat::Gif => "Animated GIF",
            ExportFormat::Mp4 => "MP4 video",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Plume => "plume",
            ExportFormat::Feather => "feather",
            ExportFormat::Obj => "obj",
            ExportFormat::Glb => "glb",
            ExportFormat::Gltf => "gltf",
            ExportFormat::Stl => "stl",
            ExportFormat::Usdz => "usdz",
            ExportFormat::Png => "png",
            ExportFormat::Gif => "gif",
            ExportFormat::Mp4 => "mp4",
        }
    }

    pub fn is_document(self) -> bool {
        matches!(self, ExportFormat::Plume | ExportFormat::Feather)
    }

    pub fn is_media(self) -> bool {
        matches!(
            self,
            ExportFormat::Png | ExportFormat::Gif | ExportFormat::Mp4
        )
    }

    pub fn is_model(self) -> bool {
        matches!(
            self,
            ExportFormat::Obj
                | ExportFormat::Glb
                | ExportFormat::Gltf
                | ExportFormat::Stl
                | ExportFormat::Usdz
        )
    }
}

#[derive(Clone, Debug)]
pub struct ExportState {
    pub format: ExportFormat,
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration: f32,
    pub quality: MeshQuality,
    pub include_guides: bool,
    pub include_hidden: bool,
    pub z_up: bool,
    pub merge: bool,
    pub unit_scale: f32,
    pub binary_stl: bool,
    pub gif_fps: u32,
    pub transparent: bool,
}

impl Default for ExportState {
    fn default() -> Self {
        Self {
            format: ExportFormat::Glb,
            path: String::new(),
            width: 1600,
            height: 1000,
            fps: 30,
            duration: 6.0,
            quality: MeshQuality::Export,
            include_guides: false,
            include_hidden: false,
            z_up: false,
            merge: false,
            unit_scale: 0.001,
            binary_stl: true,
            gif_fps: 12,
            transparent: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub error: bool,
    pub until: f64,
}

// ---------------------------------------------------------------------------
// Background media jobs
// ---------------------------------------------------------------------------

enum MediaMessage {
    Progress(f32, String),
    Done(String),
    Failed(String),
}

pub struct MediaJob {
    pub path: PathBuf,
    pub progress: f32,
    pub detail: String,
    rx: Receiver<MediaMessage>,
}

// ---------------------------------------------------------------------------
// Viewport drag state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub enum Drag {
    None,
    Orbit {
        last: Vec2,
    },
    Pan {
        last: Vec2,
    },
    Stroke {
        raw: Vec<(Vec3, f32, f64)>,
        last_screen: Vec2,
        live: Option<CurveId>,
        brush: Box<Brush>,
        config: StrokeConfig,
        normal: Vec3,
        had_surface: bool,
    },
    Shape {
        id: CurveId,
        start: Vec3,
        normal: Vec3,
        start_screen: Vec2,
        moved: bool,
    },
    Marquee {
        start: Vec2,
        current: Vec2,
        additive: bool,
    },
    Gizmo {
        axis: Axis,
        mode: TransformMode,
        start_screen: Vec2,
        centre: Vec3,
        applied: f32,
    },
    MovePlane {
        start_screen: Vec2,
        applied: Vec3,
    },
    Liquify {
        curve: CurveId,
        last_screen: Vec2,
    },
    Erase,
}

// ---------------------------------------------------------------------------
// Dirty tracking
// ---------------------------------------------------------------------------

/// Snapshot of the parts of a document that represent user content. Volatile
/// state (camera orbit, selection, modified timestamp and thumbnail) is
/// excluded so that undoing back to the last save clears the Unsaved marker.
fn document_fingerprint(document: &Document) -> String {
    let mut normalized = document.clone();
    normalized.modified.clear();
    normalized.thumbnail = None;
    normalized.camera = Camera::default();
    normalized.scene.selection.clear();
    normalized.scene.revision = 0;
    serde_json::to_string(&normalized).unwrap_or_default()
}

/// `true` when the document differs from the last saved/opened baseline.
fn differs_from_baseline(saved: Option<&str>, document: &Document) -> bool {
    match saved {
        Some(saved) => saved != document_fingerprint(document),
        None => true,
    }
}

// ---------------------------------------------------------------------------
// Application
// ---------------------------------------------------------------------------

pub struct PlumeApp {
    pub doc: Document,
    pub history: History,
    pub library: BrushLibrary,
    pub brush_id: String,
    pub brush_search: String,
    pub tool: Tool,
    pub settings: ToolSettings,
    pub left_tab: LeftTab,
    pub sandbox: Sandbox,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    saved_fingerprint: Option<String>,
    pub status: String,
    pub toast: Option<Toast>,
    pub dialog: Dialog,
    pub inspector: InspectorTab,
    pub gallery_items: Vec<GalleryItem>,
    pub gallery_selected: String,
    pub gallery_thumbs: Vec<Option<egui::TextureHandle>>,
    pub gallery_preview: Option<egui::TextureHandle>,
    pub export: ExportState,
    pub media: Option<MediaJob>,
    pub pending_confirm: Option<ConfirmAction>,
    pub drag: Drag,
    pub gesture_before: Option<Document>,
    pub edit_guard: Option<Document>,
    pub edit_guard_dirty: bool,
    pub edit_guard_label: String,
    pub last_pointer_down: bool,
    pub last_focus: bool,
    pub last_time: f64,
    pub last_ctx: Option<egui::Context>,
    /// Texture registered with egui, plus its physical pixel dimensions.
    pub view_texture: Option<TextureId>,
    pub view_size: EVec2,
    pub preview_time: Option<f32>,
    pub needs_render: bool,
    pub last_render_size: [u32; 2],
    pub viewport_size: Vec2,
    pub render_state: Option<egui_wgpu::RenderState>,
    pub renderer: Option<GpuRenderer>,
    pub quality: MeshQuality,
    pub show_guides: bool,
    pub show_stats: bool,
    pub show_timeline: bool,
    pub startup_error: Option<String>,
    pub app_started: f64,
    pub timeline_drag: bool,
    pub window_title: String,
}

impl PlumeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Option<PathBuf>) -> Result<Self> {
        theme::apply(&cc.egui_ctx);
        let sandbox = Sandbox::discover().context("could not resolve the PLUME sandbox")?;
        let mut doc = gallery::build_document("ribbon-bloom").unwrap_or_default();
        let mut startup_error = None;
        let mut path = None;
        let mut imported_startup = false;
        if let Some(startup) = startup {
            match Self::load_document_static(&sandbox, &startup) {
                Ok((mut loaded, notes, set_path)) => {
                    loaded.scene.touch();
                    doc = loaded;
                    if set_path {
                        path = Some(startup);
                    } else {
                        imported_startup = true;
                    }
                    if !notes.is_empty() {
                        startup_error = Some(notes.join(" · "));
                    }
                }
                Err(error) => {
                    startup_error =
                        Some(format!("Could not open {}: {error}", pretty_path(&startup)));
                }
            }
        }
        let render_state = cc.wgpu_render_state.clone();
        let renderer = render_state
            .as_ref()
            .map(|state| GpuRenderer::new(state.device.clone(), state.queue.clone(), [1280, 800]));
        let gallery_items = gallery::GALLERY.to_vec();
        let mut app = Self {
            doc,
            history: History::new(160),
            library: BrushLibrary::default(),
            brush_id: "sketch-medium".into(),
            brush_search: String::new(),
            tool: Tool::Brush,
            settings: ToolSettings::default(),
            left_tab: LeftTab::Tools,
            sandbox,
            path,
            dirty: false,
            saved_fingerprint: None,
            status: "Ready".into(),
            toast: None,
            dialog: Dialog::None,
            inspector: InspectorTab::Style,
            gallery_items,
            gallery_selected: "ribbon-bloom".into(),
            gallery_thumbs: Vec::new(),
            gallery_preview: None,
            export: ExportState::default(),
            media: None,
            pending_confirm: None,
            drag: Drag::None,
            gesture_before: None,
            edit_guard: None,
            edit_guard_dirty: false,
            edit_guard_label: String::new(),
            last_pointer_down: false,
            last_focus: false,
            last_time: cc.egui_ctx.input(|i| i.time),
            last_ctx: None,
            view_texture: None,
            view_size: EVec2::new(1280.0, 800.0),
            preview_time: None,
            needs_render: true,
            last_render_size: [0, 0],
            viewport_size: Vec2::new(1280.0, 800.0),
            render_state,
            renderer,
            quality: MeshQuality::Normal,
            show_guides: true,
            show_stats: true,
            show_timeline: true,
            startup_error,
            app_started: cc.egui_ctx.input(|i| i.time),
            timeline_drag: false,
            window_title: String::new(),
        };
        if let Some(brush) = app.library.get("sketch-medium").cloned() {
            app.settings.size = brush.base_size;
        }
        app.sync_export_path();
        app.saved_fingerprint = Some(document_fingerprint(&app.doc));
        // Imported mesh and Feather documents have no native PLUME file yet, so
        // their content counts as unsaved until the user saves a `.plume`.
        if imported_startup {
            app.saved_fingerprint = None;
            app.dirty = true;
        }
        if let Some(error) = app.startup_error.take() {
            app.notify(error, true);
        }
        Ok(app)
    }

    // -- basics -------------------------------------------------------------

    pub fn current_brush(&self) -> Brush {
        let mut brush = self
            .library
            .get(&self.brush_id)
            .cloned()
            .unwrap_or_else(Brush::default);
        brush.base_size = self.settings.size.max(0.2);
        brush
    }

    pub fn current_brush_ref(&self) -> Option<&Brush> {
        self.library.get(&self.brush_id)
    }

    pub fn select_brush(&mut self, id: &str) {
        if let Some(brush) = self.library.get(id).cloned() {
            self.brush_id = id.to_string();
            self.settings.size = brush.base_size;
            self.toast("Brush: ".to_string() + &brush.name, false);
            self.needs_render = true;
        }
    }

    pub fn notify(&mut self, message: impl Into<String>, error: bool) {
        let now = self.last_time.max(self.app_started);
        self.status = message.into();
        self.toast = Some(Toast {
            message: self.status.clone(),
            error,
            until: now + if error { 8.0 } else { 4.5 },
        });
    }

    pub fn toast(&mut self, message: impl Into<String>, error: bool) {
        self.notify(message, error);
    }

    pub fn touch_document(&mut self) {
        self.dirty = true;
        self.needs_render = true;
        self.doc
            .mark_modified(plume_core::io::plume::utc_timestamp());
    }

    /// Mark a property edit made through the inspector.  The frame-level edit
    /// guard captures the document before the pointer goes down, so dragging a
    /// slider produces exactly one undo entry.
    pub fn note_ui_edit(&mut self, label: &str) {
        self.touch_document();
        self.edit_guard_dirty = true;
        self.edit_guard_label = label.to_string();
    }

    // -- undo ---------------------------------------------------------------

    pub fn begin_gesture(&mut self) {
        if self.gesture_before.is_none() {
            self.gesture_before = Some(self.doc.clone());
        }
    }

    pub fn commit_gesture(&mut self, label: &str, changed: bool) {
        if let Some(before) = self.gesture_before.take() {
            if changed {
                self.history.push_snapshot(label, before);
                self.touch_document();
            }
        }
        self.drag = Drag::None;
    }

    pub fn cancel_gesture(&mut self) {
        self.gesture_before = None;
        self.drag = Drag::None;
    }

    pub fn record(&mut self, label: &str) {
        self.history.record(label, &self.doc);
    }

    pub fn undo(&mut self) {
        let camera = self.doc.camera.clone();
        if let Some(label) = self.history.undo(&mut self.doc) {
            self.doc.camera = camera;
            self.doc.scene.touch();
            self.dirty = differs_from_baseline(self.saved_fingerprint.as_deref(), &self.doc);
            self.needs_render = true;
            self.preview_time = None;
            self.notify(format!("Undo: {label}"), false);
        } else {
            self.notify("Nothing to undo", false);
        }
    }

    pub fn redo(&mut self) {
        let camera = self.doc.camera.clone();
        if let Some(label) = self.history.redo(&mut self.doc) {
            self.doc.camera = camera;
            self.doc.scene.touch();
            self.dirty = differs_from_baseline(self.saved_fingerprint.as_deref(), &self.doc);
            self.needs_render = true;
            self.preview_time = None;
            self.notify(format!("Redo: {label}"), false);
        } else {
            self.notify("Nothing to redo", false);
        }
    }

    pub fn finish_ui_frame(&mut self, ctx: &egui::Context) {
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        let focus = ctx.memory(|m| m.focused().is_some());
        let editing = pointer_down || focus;
        if editing {
            if self.edit_guard.is_none() {
                self.edit_guard = Some(self.doc.clone());
                self.edit_guard_dirty = false;
                self.edit_guard_label.clear();
            }
        } else if let Some(before) = self.edit_guard.take() {
            if self.edit_guard_dirty && !self.edit_guard_label.is_empty() {
                self.history
                    .push_snapshot(self.edit_guard_label.clone(), before);
            }
            self.edit_guard_dirty = false;
            self.edit_guard_label.clear();
        }
        self.last_pointer_down = pointer_down;
        self.last_focus = focus;
    }

    // -- document lifecycle -------------------------------------------------

    pub fn load_document_static(
        sandbox: &Sandbox,
        path: &Path,
    ) -> Result<(Document, Vec<String>, bool)> {
        if !sandbox.contains(path) {
            bail!(
                "refusing to read outside the PLUME sandbox\n{}",
                pretty_path(path)
            );
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported")
            .to_string();
        match ext.as_str() {
            "plume" => {
                let package =
                    io::load_plume(path).with_context(|| "not a readable PLUME package")?;
                let mut doc = package.document;
                doc.thumbnail = package.thumbnail;
                Ok((doc, package.notes, true))
            }
            "feather" => {
                let imported = io::import_feather(path)
                    .with_context(|| "not a readable Feather interchange archive")?;
                let mut notes = imported.notes;
                if !imported.skipped_entries.is_empty() {
                    notes.push(format!(
                        "{} unsupported Feather entries were skipped",
                        imported.skipped_entries.len()
                    ));
                }
                Ok((imported.document, notes, false))
            }
            "obj" => {
                let scene = io::import_obj(path)?;
                let doc = scene.to_document(&Brush::default(), name)?;
                Ok((doc, scene.notes, false))
            }
            "gltf" | "glb" => {
                let scene = io::import_gltf(path)?;
                let doc = scene.to_document(&Brush::default(), name)?;
                Ok((doc, scene.notes, false))
            }
            "stl" => {
                let scene = io::import_stl(path)?;
                let doc = scene.to_document(&Brush::default(), name)?;
                Ok((doc, scene.notes, false))
            }
            _ => bail!("unsupported file type “.{ext}”"),
        }
    }

    pub fn adopt_document(&mut self, mut doc: Document, path: Option<PathBuf>, notes: Vec<String>) {
        doc.scene.touch();
        self.doc = doc;
        self.path = path;
        self.history.clear();
        self.drag = Drag::None;
        self.gesture_before = None;
        self.preview_time = None;
        self.edit_guard = None;
        self.edit_guard_dirty = false;
        self.dirty = false;
        self.saved_fingerprint = Some(document_fingerprint(&self.doc));
        self.needs_render = true;
        self.last_render_size = [0, 0];
        self.gallery_preview = None;
        self.sync_export_path();
        if notes.is_empty() {
            self.notify(format!("Opened {}", self.doc.name), false);
        } else {
            self.notify(notes.join(" · "), false);
        }
        self.tool = Tool::Select;
    }

    pub fn open_path(&mut self, path: &Path) -> Result<()> {
        if !self.sandbox.contains(path) {
            bail!(
                "refusing to read outside the PLUME sandbox\n{}",
                pretty_path(path)
            );
        }
        let (doc, notes, set_path) = Self::load_document_static(&self.sandbox, path)?;
        self.adopt_document(
            doc,
            if set_path {
                Some(path.to_path_buf())
            } else {
                None
            },
            notes,
        );
        if !set_path {
            // OBJ/glTF/STL/Feather import: no native document exists on disk.
            self.saved_fingerprint = None;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn new_document(&mut self) {
        let mut doc = Document::new("Untitled");
        // A 500 mm square construction plane, centred at the origin.
        let id = doc.scene.new_guide_id();
        let mut plane = Guide::plane(id, Vec3::ZERO, Vec3::Y);
        plane.name = "Ground plane".into();
        plane.kind = GuideKind::Plane {
            width: 500.0,
            height: 500.0,
        };
        plane.transform = Mat4::from_rotation_x(-90.0f32.to_radians());
        plane.solid = true;
        plane.opacity = 0.22;
        plane.rebuild();
        doc.scene.add_guide(plane);
        doc.scene.set_active_guide(Some(id));
        self.adopt_document(doc, None, Vec::new());
        self.notify("New sketch", false);
        self.needs_render = true;
    }

    pub fn load_gallery(&mut self, id: &str) {
        if let Some(mut doc) = gallery::build_document(id) {
            doc.scene.touch();
            let name = doc.name.clone();
            self.adopt_document(doc, None, Vec::new());
            self.gallery_selected = id.to_string();
            self.dirty = false;
            self.notify(format!("Loaded gallery scene “{name}”"), false);
        } else {
            self.notify(format!("Unknown gallery scene “{id}”"), true);
        }
    }

    pub fn sync_export_path(&mut self) {
        let ext = self.export.format.extension();
        let stem = crate::sandbox::sanitize_stem(&self.doc.name);
        let base = self.sandbox.projects_dir().join(format!("{stem}.{ext}"));
        self.export.path = pretty_path(&base);
        self.export.duration = self.doc.animation.effective_duration().clamp(0.5, 120.0);
    }

    pub fn choose_open_path(&mut self) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Open a PLUME scene")
            .set_directory(self.sandbox.projects_dir());
        for (name, ext) in [
            ("PLUME documents", &["plume"][..]),
            ("Feather documents", &["feather"][..]),
            ("3D meshes", &["obj", "gltf", "glb", "stl"][..]),
            ("All files", &["*"][..]),
        ] {
            dialog = dialog.add_filter(name, ext);
        }
        let path = dialog.pick_file()?;
        if !self.sandbox.contains(&path) {
            self.notify(
                "That file is outside the PLUME sandbox. Copy it into the project folder first.",
                true,
            );
            return None;
        }
        Some(path)
    }

    pub fn choose_save_path(&mut self, ext: &str, title: &str) -> Option<PathBuf> {
        let suggested = self.sandbox.projects_dir().join(format!(
            "{}.{}",
            crate::sandbox::sanitize_stem(&self.doc.name),
            ext
        ));
        let path = rfd::FileDialog::new()
            .set_title(title)
            .set_directory(self.sandbox.projects_dir())
            .set_file_name(
                suggested
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Untitled.plume"),
            )
            .add_filter("PLUME scene", &["plume"])
            .add_filter("Feather scene", &["feather"])
            .save_file()?;
        match self.sandbox.ensure_file(&path) {
            Ok(path) => Some(path),
            Err(error) => {
                self.notify(format!("{error:#}"), true);
                None
            }
        }
    }

    /// Save the document. Returns `false` if the user cancelled a file dialog.
    pub fn save_current(&mut self) -> bool {
        let path = if let Some(path) = self.path.clone() {
            path
        } else {
            match self.choose_save_path("plume", "Save PLUME scene") {
                Some(path) => path,
                None => return false,
            }
        };
        match self.save_to(&path) {
            Ok(()) => true,
            Err(error) => {
                self.notify(format!("{error:#}"), true);
                false
            }
        }
    }

    pub fn save_as(&mut self) -> bool {
        let Some(path) = self.choose_save_path("plume", "Save PLUME scene as") else {
            return false;
        };
        match self.save_to(&path) {
            Ok(()) => true,
            Err(error) => {
                self.notify(format!("{error:#}"), true);
                false
            }
        }
    }

    pub fn save_to(&mut self, path: &Path) -> Result<()> {
        let path = self.sandbox.ensure_file(path)?;
        let thumbnail = gallery::thumbnail(&self.doc, 512, 320);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext == "feather" {
            io::export_feather(&self.doc, &path, Some(&thumbnail))?;
        } else {
            io::save_plume(&path, &self.doc, Some(&thumbnail), &BTreeMap::new())?;
        }
        self.doc.modified = plume_core::io::plume::utc_timestamp();
        self.path = Some(path.clone());
        self.dirty = false;
        self.saved_fingerprint = Some(document_fingerprint(&self.doc));
        self.notify(format!("Saved {}", pretty_path(&path)), false);
        Ok(())
    }

    pub fn request_new(&mut self) {
        if self.dirty {
            self.pending_confirm = Some(ConfirmAction::New);
        } else {
            self.new_document();
        }
    }

    pub fn request_open(&mut self) {
        if self.dirty {
            self.pending_confirm = Some(ConfirmAction::Open);
            return;
        }
        if let Some(path) = self.choose_open_path() {
            if let Err(error) = self.open_path(&path) {
                self.notify(format!("{error:#}"), true);
            }
        }
    }

    pub fn request_gallery(&mut self, id: &str) {
        if self.dirty {
            self.pending_confirm = Some(ConfirmAction::Gallery(id.to_string()));
        } else {
            self.load_gallery(id);
            self.dialog = Dialog::None;
        }
    }

    pub fn apply_confirm(&mut self, action: ConfirmAction, save: bool) {
        if save && !self.save_current() {
            return;
        }
        match action {
            ConfirmAction::New => {
                self.new_document();
                self.dialog = Dialog::None;
            }
            ConfirmAction::Open => {
                if let Some(path) = self.choose_open_path() {
                    if let Err(error) = self.open_path(&path) {
                        self.notify(format!("{error:#}"), true);
                    }
                }
            }
            ConfirmAction::Gallery(id) => {
                self.load_gallery(&id);
                self.dialog = Dialog::None;
            }
            ConfirmAction::Quit => {
                self.notify("Closing PLUME", false);
                if let Some(ctx) = self.last_ctx.as_ref() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    // -- import/export ------------------------------------------------------

    pub fn import_mesh_dialog(&mut self) {
        let path = rfd::FileDialog::new()
            .set_title("Import 3D mesh as guide surfaces")
            .set_directory(self.sandbox.projects_dir())
            .add_filter("3D meshes", &["obj", "gltf", "glb", "stl"])
            .pick_file();
        let Some(path) = path else { return };
        if !self.sandbox.contains(&path) {
            self.notify("That file is outside the PLUME sandbox.", true);
            return;
        }
        let scene = match path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "obj" => io::import_obj(&path),
            "gltf" | "glb" => io::import_gltf(&path),
            "stl" => io::import_stl(&path),
            _ => {
                self.notify("Unsupported mesh format", true);
                return;
            }
        };
        match scene {
            Ok(scene) => {
                if scene.meshes.is_empty() {
                    self.notify("The file contains no geometry", true);
                    return;
                }
                let before = self.doc.clone();
                let count = scene.meshes.len();
                for imported in &scene.meshes {
                    let id = self.doc.scene.new_guide_id();
                    let guide = Guide::imported(
                        id,
                        if imported.name.trim().is_empty() {
                            "Imported mesh".to_string()
                        } else {
                            imported.name.clone()
                        },
                        imported.mesh.clone(),
                        Some(pretty_path(&path)),
                    );
                    self.doc.scene.add_guide(guide);
                }
                self.history.push_snapshot("Import mesh", before);
                self.touch_document();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("mesh");
                self.notify(format!("Imported {count} object(s) from {name}"), false);
            }
            Err(error) => self.notify(format!("Import failed: {error:#}"), true),
        }
    }

    pub fn run_export(&mut self) {
        let path = PathBuf::from(self.export.path.clone());
        if path.as_os_str().is_empty() {
            self.notify("Choose an export destination first", true);
            return;
        }
        if self.media.is_some() {
            self.notify("A capture is already running", true);
            return;
        }
        let path = match self.sandbox.ensure_file(&path) {
            Ok(path) => path,
            Err(error) => {
                self.notify(format!("{error:#}"), true);
                return;
            }
        };
        let format = self.export.format;
        if format.is_media() {
            self.start_media_job(path, format);
            return;
        }
        let result = match format {
            ExportFormat::Plume => {
                let thumb = gallery::thumbnail(&self.doc, 512, 320);
                io::save_plume(&path, &self.doc, Some(&thumb), &BTreeMap::new())
                    .map(|_| "PLUME package".to_string())
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Feather => {
                let thumb = gallery::thumbnail(&self.doc, 512, 320);
                io::export_feather(&self.doc, &path, Some(&thumb))
                    .map(|_| "Feather-compatible package".to_string())
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Obj => {
                let options = self.export_options();
                io::export_obj(&self.doc, &path, &options)
                    .map(|report| {
                        format!(
                            "{} · {} triangles",
                            human_bytes(report.bytes),
                            report.triangles
                        )
                    })
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Glb => {
                let options = self.export_options();
                io::export_glb(&self.doc, &path, &options)
                    .map(|report| {
                        format!(
                            "{} · {} triangles",
                            human_bytes(report.bytes),
                            report.triangles
                        )
                    })
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Gltf => {
                let options = self.export_options();
                io::export_gltf(&self.doc, &path, &options)
                    .map(|report| {
                        format!(
                            "{} · {} triangles",
                            human_bytes(report.bytes),
                            report.triangles
                        )
                    })
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Stl => {
                let options = self.export_options();
                io::export_stl(&self.doc, &path, &options, self.export.binary_stl)
                    .map(|report| {
                        format!(
                            "{} · {} triangles",
                            human_bytes(report.bytes),
                            report.triangles
                        )
                    })
                    .map_err(anyhow::Error::from)
            }
            ExportFormat::Usdz => {
                let options = self.export_options();
                io::export_usdz(&self.doc, &path, &options)
                    .map(|report| {
                        format!(
                            "{} · {} triangles",
                            human_bytes(report.bytes),
                            report.triangles
                        )
                    })
                    .map_err(anyhow::Error::from)
            }
            _ => unreachable!(),
        };
        match result {
            Ok(detail) => {
                self.notify(format!("Exported {} · {detail}", pretty_path(&path)), false);
                self.dialog = Dialog::None;
            }
            Err(error) => self.notify(format!("Export failed: {error:#}"), true),
        }
    }

    fn export_options(&self) -> ExportOptions {
        ExportOptions {
            quality: self.export.quality,
            include_guides: self.export.include_guides,
            include_hidden: self.export.include_hidden,
            unit_scale: self.export.unit_scale,
            z_up: self.export.z_up,
            merge: self.export.merge,
            double_sided: true,
        }
    }

    fn start_media_job(&mut self, path: PathBuf, format: ExportFormat) {
        let (width, height) = match format {
            ExportFormat::Png => (
                self.export.width.clamp(128, 8192),
                self.export.height.clamp(128, 8192),
            ),
            ExportFormat::Gif => (
                self.export.width.clamp(128, 1280),
                self.export.height.clamp(128, 960),
            ),
            _ => (
                self.export.width.clamp(128, 3840),
                self.export.height.clamp(128, 2160),
            ),
        };
        let fps = if format == ExportFormat::Gif {
            self.export.gif_fps.clamp(2, 30)
        } else {
            self.export.fps.clamp(1, 120)
        };
        let duration = self.export.duration.clamp(0.2, 120.0);
        let mut frames = if format == ExportFormat::Png {
            1
        } else {
            (duration * fps as f32).round() as u32
        };
        if format == ExportFormat::Gif {
            frames = frames.min(240);
        }
        if format == ExportFormat::Mp4 {
            frames = frames.min(3600);
        }
        let quality = self.export.quality;
        let doc = self.doc.clone();
        let label = format!(
            "{} · {}×{} · {frames} frame(s)",
            format.label(),
            width,
            height
        );
        let (tx, rx) = mpsc::channel();
        let job_tx = tx.clone();
        let job_path = path.clone();
        let job_label = label.clone();
        let job_delay = 1000 / fps.max(1);
        let spawn = std::thread::Builder::new()
            .name("plume-media".into())
            .spawn(move || {
                let result = render_media_job(
                    doc,
                    job_path,
                    format,
                    width,
                    height,
                    quality,
                    frames,
                    fps,
                    job_delay,
                    job_tx.clone(),
                );
                match result {
                    Ok(detail) => {
                        let _ = job_tx.send(MediaMessage::Done(detail));
                    }
                    Err(error) => {
                        let _ = job_tx.send(MediaMessage::Failed(format!("{error:#}")));
                    }
                }
            });
        if let Err(error) = spawn {
            self.notify(format!("Could not start capture: {error}"), true);
            return;
        }
        self.notify(format!("Started {job_label}"), false);
        self.media = Some(MediaJob {
            path,
            progress: 0.0,
            detail: "Starting".into(),
            rx,
        });
    }

    pub fn poll_media(&mut self) {
        let Some(job) = self.media.as_mut() else {
            return;
        };
        let mut finished = false;
        let mut toast: Option<(String, bool)> = None;
        while let Ok(message) = job.rx.try_recv() {
            match message {
                MediaMessage::Progress(progress, detail) => {
                    job.progress = progress;
                    job.detail = detail;
                }
                MediaMessage::Done(detail) => {
                    let path = job.path.clone();
                    job.progress = 1.0;
                    job.detail = detail.clone();
                    finished = true;
                    toast = Some((format!("Exported {} · {detail}", pretty_path(&path)), false));
                }
                MediaMessage::Failed(error) => {
                    job.detail = error.clone();
                    job.progress = 0.0;
                    finished = true;
                    toast = Some((format!("Capture failed: {error}"), true));
                }
            }
        }
        if finished {
            self.media = None;
            self.dialog = Dialog::None;
        }
        if let Some((message, error)) = toast {
            self.toast(message, error);
        }
    }

    // -- gallery ------------------------------------------------------------

    pub fn ensure_gallery_thumbnails(&mut self, ctx: &egui::Context) {
        if self.gallery_thumbs.len() == self.gallery_items.len() {
            return;
        }
        self.gallery_thumbs = self
            .gallery_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let doc = gallery::build_document(item.id)?;
                let image = gallery::thumbnail(&doc, 360, 220);
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                );
                Some(ctx.load_texture(
                    format!("plume-gallery-{index}"),
                    color,
                    egui::TextureOptions::LINEAR,
                ))
            })
            .collect();
    }

    pub fn ensure_gallery_preview(&mut self, ctx: &egui::Context) {
        let selected = self.gallery_selected.clone();
        if self.gallery_preview.is_some() {
            return;
        }
        if let Some(doc) = gallery::build_document(&selected) {
            let image = gallery::thumbnail(&doc, 720, 440);
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            self.gallery_preview = Some(ctx.load_texture(
                format!("plume-gallery-preview-{selected}"),
                color,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    pub fn refresh_gallery_preview(&mut self, ctx: &egui::Context) {
        self.gallery_preview = None;
        self.ensure_gallery_preview(ctx);
    }

    // -- viewport helpers ---------------------------------------------------

    pub fn view_camera(&self) -> Camera {
        if let Some(time) = self.preview_time {
            self.doc.animation.camera_at(&self.doc.camera, time)
        } else {
            self.doc.camera.clone()
        }
    }

    pub fn exit_preview(&mut self) {
        if self.preview_time.take().is_some() {
            self.doc.animation.pause();
            self.needs_render = true;
        }
    }

    pub fn set_preview_time(&mut self, time: f32) {
        let time = time.clamp(0.0, self.doc.animation.effective_duration());
        self.doc.animation.time = time;
        self.preview_time = Some(time);
        self.needs_render = true;
    }

    pub fn toggle_playback(&mut self) {
        if self.doc.animation.playing {
            self.doc.animation.pause();
            self.preview_time = Some(self.doc.animation.time);
        } else {
            self.doc.animation.play();
            self.preview_time = Some(self.doc.animation.time);
            self.needs_render = true;
        }
    }

    pub fn stop_playback(&mut self) {
        self.doc.animation.stop();
        self.preview_time = None;
        self.needs_render = true;
    }

    pub fn frame_view(&mut self) {
        let aspect = if self.viewport_size.y > 1.0 {
            self.viewport_size.x / self.viewport_size.y
        } else {
            1.6
        };
        let (min, max) = self.doc.scene.bounds_visible();
        let (min, max) = if (max - min).length() < 1.0 {
            (Vec3::splat(-80.0), Vec3::splat(80.0))
        } else {
            (min, max)
        };
        self.exit_preview();
        self.doc.camera.frame_bounds(min, max, aspect);
        self.needs_render = true;
        self.notify("Framed visible geometry", false);
    }

    pub fn frame_selection(&mut self) {
        if let Some((min, max)) = self.doc.scene.selection_bounds() {
            let aspect = if self.viewport_size.y > 1.0 {
                self.viewport_size.x / self.viewport_size.y
            } else {
                1.6
            };
            self.exit_preview();
            self.doc.camera.frame_bounds(min, max, aspect);
            self.needs_render = true;
        } else {
            self.frame_view();
        }
    }

    pub fn snap_view(&mut self, snap: ViewSnap) {
        self.exit_preview();
        self.doc.camera.snap_to(snap);
        self.needs_render = true;
        self.status = format!("View: {}", snap.label());
    }

    pub fn delete_selection(&mut self) {
        if self.doc.scene.selection.is_empty() {
            self.notify("Nothing selected", false);
            return;
        }
        let before = self.doc.clone();
        let count = self.doc.scene.delete_selection();
        if count > 0 {
            self.history.push_snapshot("Delete selection", before);
            self.touch_document();
            self.notify(format!("Deleted {count} object(s)"), false);
        }
    }

    pub fn duplicate_selection(&mut self) {
        if self.doc.scene.selection.curves.is_empty() {
            self.notify("Select one or more strokes to duplicate", false);
            return;
        }
        let before = self.doc.clone();
        let created = self.doc.scene.duplicate_selection();
        if !created.is_empty() {
            self.history.push_snapshot("Duplicate", before);
            self.touch_document();
            self.notify(format!("Duplicated {} stroke(s)", created.len()), false);
        }
    }

    pub fn group_selection(&mut self) {
        if self.doc.scene.selection.curves.is_empty() {
            self.notify("Select strokes to group", false);
            return;
        }
        let before = self.doc.clone();
        let name = format!("Group {}", self.doc.scene.groups.len() + 1);
        if let Some(id) = self.doc.scene.group_selection(name) {
            self.history.push_snapshot("Group selection", before);
            self.doc.scene.selection.groups.insert(id);
            self.touch_document();
            self.notify("Created group", false);
        }
    }

    pub fn ungroup_selection(&mut self) {
        let before = self.doc.clone();
        let count = self.doc.scene.ungroup_selection();
        if count > 0 {
            self.history.push_snapshot("Ungroup", before);
            self.touch_document();
            self.notify(format!("Ungrouped {count} object(s)"), false);
        }
    }

    pub fn mirror_selection(&mut self) {
        if self.doc.scene.selection.curves.is_empty() {
            self.notify("Select strokes to mirror", false);
            return;
        }
        let before = self.doc.clone();
        let created = self.doc.scene.mirror_selection();
        if created.is_empty() {
            self.notify("Nothing was mirrored", false);
            return;
        }
        self.history.push_snapshot("Mirror", before);
        self.touch_document();
        self.notify(format!("Mirrored {} stroke(s)", created.len()), false);
    }

    pub fn add_default_guide(&mut self, kind: GuideKind) {
        let centre = self
            .doc
            .scene
            .selection_centre()
            .unwrap_or(self.doc.camera.target);
        let size = (self.doc.camera.distance() * 0.35).clamp(20.0, 600.0);
        let (name, guide_kind) = match kind {
            GuideKind::Plane { .. } => (
                "Construction plane",
                GuideKind::Plane {
                    width: size * 1.6,
                    height: size * 1.6,
                },
            ),
            GuideKind::Sphere { .. } => (
                "Guide sphere",
                GuideKind::Sphere {
                    radius: size,
                    segments: 48,
                    rings: 24,
                },
            ),
            GuideKind::Box { .. } => (
                "Guide box",
                GuideKind::Box {
                    size: Vec3::new(size * 1.4, size * 1.1, size * 1.4),
                },
            ),
            GuideKind::Cylinder { .. } => (
                "Guide cylinder",
                GuideKind::Cylinder {
                    radius: size * 0.65,
                    height: size * 1.7,
                    segments: 48,
                },
            ),
            GuideKind::Cone { .. } => (
                "Guide cone",
                GuideKind::Cone {
                    radius: size * 0.75,
                    height: size * 1.7,
                    segments: 48,
                },
            ),
            GuideKind::Torus { .. } => (
                "Guide torus",
                GuideKind::Torus {
                    major_radius: size * 0.8,
                    minor_radius: size * 0.24,
                    major_segments: 64,
                    minor_segments: 20,
                },
            ),
            _ => ("Guide", kind),
        };
        let before = self.doc.clone();
        let id = self.doc.scene.new_guide_id();
        let mut guide = Guide::new(id, name, guide_kind);
        guide.transform = Mat4::from_translation(centre);
        guide.solid = true;
        guide.opacity = 0.30;
        guide.rebuild();
        self.doc.scene.add_guide(guide);
        self.doc.scene.set_active_guide(Some(id));
        self.history.push_snapshot("Add guide", before);
        self.touch_document();
        self.notify(format!("Added {name}"), false);
    }

    pub fn place_plane_at(&mut self, point: Vec3, normal: Vec3) {
        let before = self.doc.clone();
        let id = self.doc.scene.new_guide_id();
        let mut guide = Guide::plane(id, Vec3::ZERO, Vec3::Y);
        guide.name = "Construction plane".into();
        guide.kind = GuideKind::Plane {
            width: (self.doc.camera.distance() * 0.9).clamp(60.0, 800.0),
            height: (self.doc.camera.distance() * 0.9).clamp(60.0, 800.0),
        };
        let normal = normal.normalize_or_zero();
        if normal.length_squared() > 1.0e-6 {
            guide.transform = Mat4::from_translation(point) * rotation_from_y(normal);
        } else {
            guide.transform = Mat4::from_translation(point);
        }
        guide.solid = true;
        guide.opacity = 0.28;
        guide.rebuild();
        self.doc.scene.add_guide(guide);
        self.doc.scene.set_active_guide(Some(id));
        self.history.push_snapshot("Construction plane", before);
        self.touch_document();
        self.notify("Placed construction plane", false);
    }

    pub fn render_view(&mut self, ctx: &egui::Context, rect: Rect) -> Option<TextureId> {
        let render_state = self.render_state.clone()?;
        let ppp = ctx.pixels_per_point();
        let width = (rect.width() * ppp).round().clamp(16.0, 4096.0) as u32;
        let height = (rect.height() * ppp).round().clamp(16.0, 4096.0) as u32;
        let resized = self.last_render_size != [width, height];
        let playing = self.doc.animation.playing;
        if !(resized || self.needs_render || playing) {
            return self.renderer.as_ref().and(self.view_texture);
        }
        let render_at = [width, height];
        let mut temp_doc;
        let doc_ref = if self.preview_time.is_some() || !self.show_guides {
            temp_doc = self.doc.clone();
            if let Some(time) = self.preview_time {
                temp_doc.camera = self.doc.animation.camera_at(&self.doc.camera, time);
            }
            if !self.show_guides {
                for guide in &mut temp_doc.scene.guides {
                    guide.visible = false;
                }
                temp_doc.scene.revision = temp_doc.scene.revision.wrapping_add(1).max(1);
            }
            &temp_doc
        } else {
            &self.doc
        };
        let time = self.preview_time.unwrap_or(0.0);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.render(doc_ref, render_at, self.quality, time);
            let id = renderer.texture_id(&render_state);
            self.view_texture = Some(id);
            self.view_size = EVec2::new(width as f32, height as f32);
            self.last_render_size = render_at;
            self.needs_render = false;
            Some(id)
        } else {
            None
        }
    }

    pub fn viewport_response(&mut self, ui: &mut egui::Ui, rect: Rect) -> egui::Response {
        ui.interact(
            rect,
            ui.scope_id().with("viewport"),
            egui::Sense::click_and_drag(),
        )
    }

    // -- input --------------------------------------------------------------

    pub fn handle_viewport_input(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        response: &egui::Response,
    ) {
        let ctx = ui.ctx().clone();
        let camera = self.view_camera();
        let size = Vec2::new(rect.width().max(1.0), rect.height().max(1.0));
        self.viewport_size = size;
        let pointer = ctx.input(|i| i.pointer.interact_pos());
        let Some(pointer) = pointer else { return };
        if !rect.contains(pointer) {
            return;
        }
        let local = Vec2::new(pointer.x - rect.left(), pointer.y - rect.top());
        let top_strip = rect.top() + 40.0;
        if pointer.y < top_strip {
            return;
        }
        let alt = ctx.input(|i| i.modifiers.alt);
        let shift = ctx.input(|i| i.modifiers.shift);
        let primary_down = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Primary));
        let secondary_down = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Secondary));
        let middle_down = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
        let primary_pressed = ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        let secondary_pressed =
            ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary));
        let middle_pressed = ctx.input(|i| i.pointer.button_pressed(egui::PointerButton::Middle));
        let primary_released =
            ctx.input(|i| i.pointer.button_released(egui::PointerButton::Primary));
        let duplicate_click = ctx.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
        });
        let _ = duplicate_click;

        // Wheel zoom, always available while hovering the viewport.
        let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.01 && response.hovered() {
            self.exit_preview();
            let mut camera = camera.clone();
            camera.zoom((scroll * 0.0015).exp());
            self.doc.camera = camera;
            self.needs_render = true;
        }

        // Secondary / middle navigation has priority over tools.
        if secondary_pressed && (self.drag_matches_nav() || matches!(self.drag, Drag::None)) {
            self.exit_preview();
            self.drag = if shift {
                Drag::Pan { last: local }
            } else {
                Drag::Orbit { last: local }
            };
            return;
        }
        if middle_pressed {
            self.exit_preview();
            self.drag = Drag::Pan { last: local };
            return;
        }
        if primary_pressed && alt && matches!(self.drag, Drag::None) {
            self.exit_preview();
            self.drag = Drag::Orbit { last: local };
            return;
        }

        // Continue navigation.
        match &mut self.drag {
            Drag::Orbit { last } => {
                if secondary_down || primary_down {
                    let delta = local - *last;
                    *last = local;
                    let mut camera = self.doc.camera.clone();
                    camera.orbit_pixels(delta.x, delta.y);
                    self.doc.camera = camera;
                    self.needs_render = true;
                } else {
                    self.drag = Drag::None;
                }
                return;
            }
            Drag::Pan { last } => {
                if secondary_down || middle_down || primary_down {
                    let delta = local - *last;
                    *last = local;
                    let mut camera = self.doc.camera.clone();
                    camera.pan_pixels(delta.x, delta.y, size);
                    self.doc.camera = camera;
                    self.needs_render = true;
                } else {
                    self.drag = Drag::None;
                }
                return;
            }
            _ => {}
        }

        if primary_pressed && matches!(self.drag, Drag::None) {
            self.exit_preview();
            self.begin_gesture();
            match self.tool {
                Tool::Brush => self.begin_stroke(&camera, size, local),
                Tool::Erase => self.begin_erase(&camera, size, local),
                Tool::Liquify => self.begin_liquify(&camera, size, local),
                Tool::Select => self.begin_select(&camera, size, local, shift, alt),
                Tool::Move => self.begin_move(&camera, size, local, shift),
                Tool::Guide => self.place_guide_at(&camera, size, local),
                Tool::Line | Tool::Circle | Tool::Ellipse | Tool::Polygon => {
                    self.begin_shape(&camera, size, local)
                }
            }
            return;
        }

        if primary_down {
            match self.tool {
                Tool::Brush => self.update_stroke(&camera, size, local),
                Tool::Erase => self.update_erase(&camera, size, local),
                Tool::Liquify => self.update_liquify(&camera, size, local),
                Tool::Select => self.update_select(&camera, size, local),
                Tool::Move => self.update_move(&camera, size, local, shift),
                Tool::Line | Tool::Circle | Tool::Ellipse | Tool::Polygon => {
                    self.update_shape(&camera, size, local)
                }
                _ => {}
            }
        }

        if primary_released {
            if matches!(self.tool, Tool::Select) {
                self.finish_select(&camera, size, local, shift);
            } else if matches!(
                self.tool,
                Tool::Brush | Tool::Erase | Tool::Liquify | Tool::Move
            ) {
                self.finish_edit_tool();
            } else if self.tool.is_shape() {
                self.finish_shape(&camera, size, local);
            } else if self.tool == Tool::Guide {
                self.commit_gesture("Construction plane", false);
            }
        }
    }

    fn drag_matches_nav(&self) -> bool {
        matches!(self.drag, Drag::Orbit { .. } | Drag::Pan { .. })
    }

    fn begin_stroke(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let plane = self.drawing_plane(camera);
        let mut guides = std::mem::take(&mut self.doc.scene.guides);
        let point = capture::project_to_surface(local, camera, size, &mut guides, plane);
        self.doc.scene.guides = guides;
        let Some(point) = point else {
            self.cancel_gesture();
            return;
        };
        let brush = self.current_brush();
        let config = StrokeConfig {
            spacing: self.settings.spacing.max(0.2),
            smoothing: self.settings.smoothing.clamp(0.0, 0.95),
            stable_stroke: self.settings.stable_stroke,
            stable_strength: 0.75,
            minimum_length: 0.4,
        };
        self.drag = Drag::Stroke {
            raw: vec![(point.position, self.settings.pressure, 0.0)],
            last_screen: local,
            live: None,
            brush: Box::new(brush),
            config,
            normal: point.normal,
            had_surface: point.guide_index.is_some(),
        };
        self.needs_render = true;
    }

    fn update_stroke(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        // Clone the small stroke state so the rest of the function can borrow
        // the document (and its guides) mutably without aliasing `self.drag`.
        let (mut raw, last_screen, live, normal, had_surface, brush, config) = match &self.drag {
            Drag::Stroke {
                raw,
                last_screen,
                live,
                brush,
                config,
                normal,
                had_surface,
            } => (
                raw.clone(),
                *last_screen,
                *live,
                *normal,
                *had_surface,
                brush.clone(),
                config.clone(),
            ),
            _ => return,
        };
        if local.distance(last_screen) < 0.7 {
            return;
        }
        let plane = self.drawing_plane(camera);
        let pressure = self.settings.pressure;
        let mut guides = std::mem::take(&mut self.doc.scene.guides);
        let point = capture::project_to_surface(local, camera, size, &mut guides, plane);
        self.doc.scene.guides = guides;
        let Some(point) = point else { return };
        raw.push((point.position, pressure, (raw.len() as f64) * 0.008));
        let mut live = live;
        if raw.len() >= 2 {
            let id = match live {
                Some(id) if self.doc.scene.curve(id).is_some() => id,
                _ => self.doc.scene.new_curve_id(),
            };
            if let Some(mut curve) = capture::build_curve_from_world(id, &raw, &brush, &config) {
                curve.points.iter_mut().for_each(|p| {
                    p.timestamp = 0.0;
                });
                if self.doc.scene.curve(id).is_some() {
                    if let Some(existing) = self.doc.scene.curve_mut(id) {
                        existing.points = curve.points;
                        existing.refresh_frames();
                    }
                    live = Some(id);
                } else {
                    live = Some(self.doc.scene.add_curve(curve));
                }
            }
        }
        self.drag = Drag::Stroke {
            raw,
            last_screen: local,
            live,
            brush,
            config,
            normal,
            had_surface,
        };
        self.needs_render = true;
    }

    fn finish_stroke(&mut self) {
        let had_curve = match &self.drag {
            Drag::Stroke { live, .. } => live.is_some(),
            _ => false,
        };
        if had_curve {
            if let Some(curve_id) = match &self.drag {
                Drag::Stroke { live, .. } => *live,
                _ => None,
            } {
                if let Some(curve) = self.doc.scene.curve(curve_id).cloned() {
                    let mirrored = self.doc.scene.mirror_curve_if_enabled(&curve);
                    if let Some(mirror_id) = mirrored {
                        let _ = mirror_id;
                    }
                }
            }
        }
        self.commit_gesture("Brush stroke", had_curve);
    }

    fn begin_erase(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let settings = PickSettings {
            tolerance: 10.0,
            ..PickSettings::default()
        };
        let result = selection::pick(&self.doc.scene, camera, size, local, &settings);
        if result.curve.is_none() {
            self.cancel_gesture();
            return;
        }
        self.drag = Drag::Erase;
        self.update_erase(camera, size, local);
    }

    fn update_erase(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let settings = PickSettings {
            tolerance: 12.0,
            ..PickSettings::default()
        };
        let result = selection::pick(&self.doc.scene, camera, size, local, &settings);
        if let Some(id) = result.curve {
            if self.doc.scene.remove_curve(id).is_some() {
                self.doc.scene.selection.curves.remove(&id);
                self.needs_render = true;
                self.touch_document();
            }
        }
    }

    fn begin_liquify(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let settings = PickSettings {
            tolerance: 24.0,
            ..PickSettings::default()
        };
        let result = selection::pick(&self.doc.scene, camera, size, local, &settings);
        let Some(id) = result.curve else {
            self.cancel_gesture();
            return;
        };
        self.doc.scene.selection.only_curve(id);
        self.drag = Drag::Liquify {
            curve: id,
            last_screen: local,
        };
    }

    fn update_liquify(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let (curve_id, last) = match &mut self.drag {
            Drag::Liquify { curve, last_screen } => (*curve, *last_screen),
            _ => return,
        };
        let delta = local - last;
        if delta.length_squared() < 0.01 {
            return;
        }
        let liquify = self.settings.liquify.clone();
        let mut moved = 0;
        if let Some(curve) = self.doc.scene.curve_mut(curve_id) {
            moved = liquify::apply_screen_liquify(
                curve,
                camera,
                size,
                local,
                delta,
                liquify.radius_px,
                liquify.mode,
                liquify.strength,
                liquify.falloff,
                self.settings.pressure,
            );
        }
        if moved > 0 {
            if let Some(curve) = self.doc.scene.curve_mut(curve_id) {
                curve.refresh_frames();
            }
            if let Drag::Liquify { last_screen, .. } = &mut self.drag {
                *last_screen = local;
            }
            self.touch_document();
        }
    }

    fn begin_select(&mut self, camera: &Camera, size: Vec2, local: Vec2, shift: bool, alt: bool) {
        let settings = PickSettings {
            guides_first: alt,
            ..PickSettings::default()
        };
        let result = selection::pick(&self.doc.scene, camera, size, local, &settings);
        if let Some(id) = result.curve {
            if !shift && !self.doc.scene.selection.curves.contains(&id) {
                self.doc.scene.selection.only_curve(id);
            }
            self.cancel_gesture();
            self.needs_render = true;
            return;
        }
        if let Some(id) = result.guide {
            if shift {
                self.doc.scene.selection.toggle_guide(id);
            } else {
                self.doc.scene.selection.only_guide(id);
            }
            self.doc.scene.set_active_guide(Some(id));
            self.cancel_gesture();
            self.needs_render = true;
            return;
        }
        if !shift {
            self.doc.scene.selection.clear();
        }
        self.drag = Drag::Marquee {
            start: local,
            current: local,
            additive: shift,
        };
        // Selection changes are not undoable, so the gesture snapshot is not
        // needed for a marquee.
        self.gesture_before = None;
        self.needs_render = true;
    }

    fn update_select(&mut self, _camera: &Camera, _size: Vec2, local: Vec2) {
        if let Drag::Marquee { current, .. } = &mut self.drag {
            *current = local;
        }
    }

    fn finish_select(&mut self, camera: &Camera, size: Vec2, local: Vec2, _shift: bool) {
        if let Drag::Marquee {
            start,
            current,
            additive,
        } = self.drag.clone()
        {
            let end = if local.distance(start) > 2.0 {
                local
            } else {
                current
            };
            if start.distance(end) > 2.0 {
                let hits = selection::marquee(
                    &self.doc.scene,
                    camera,
                    size,
                    start.min(end),
                    start.max(end),
                );
                if !additive {
                    self.doc.scene.selection.clear();
                }
                for id in hits {
                    self.doc.scene.selection.curves.insert(id);
                }
            }
        }
        self.drag = Drag::None;
        self.gesture_before = None;
        self.needs_render = true;
    }

    fn begin_move(&mut self, camera: &Camera, size: Vec2, local: Vec2, _shift: bool) {
        let centre = self.doc.scene.selection_centre();
        if let Some(centre) = centre {
            let handles =
                transform::gizmo_handles(centre, camera, size, self.settings.transform_mode);
            if let Some(handle) = transform::hit_gizmo(&handles, local, 13.0) {
                self.drag = Drag::Gizmo {
                    axis: handle.axis,
                    mode: self.settings.transform_mode,
                    start_screen: local,
                    centre,
                    applied: 0.0,
                };
                return;
            }
        }
        let settings = PickSettings {
            tolerance: 10.0,
            ..PickSettings::default()
        };
        let result = selection::pick(&self.doc.scene, camera, size, local, &settings);
        if let Some(id) = result.curve {
            if !self.doc.scene.selection.curves.contains(&id) {
                self.doc.scene.selection.only_curve(id);
            }
            self.drag = Drag::MovePlane {
                start_screen: local,
                applied: Vec3::ZERO,
            };
        } else {
            self.cancel_gesture();
        }
        self.needs_render = true;
    }

    fn update_move(&mut self, camera: &Camera, size: Vec2, local: Vec2, _shift: bool) {
        match self.drag.clone() {
            Drag::Gizmo {
                axis,
                mode,
                start_screen,
                centre,
                applied,
            } => {
                let amount = transform::axis_drag_amount(
                    camera,
                    size,
                    centre,
                    axis.vector(),
                    start_screen,
                    local,
                );
                let delta = amount - applied;
                if delta.abs() < 1.0e-5 {
                    return;
                }
                let matrix = match mode {
                    TransformMode::Translate => transform::translate_matrix(axis, delta),
                    TransformMode::Rotate => {
                        let snapped = self.settings.snap.rotation(amount);
                        let prev = self.settings.snap.rotation(applied);
                        transform::rotate_matrix(axis, snapped - prev, centre)
                    }
                    TransformMode::Scale => {
                        let factor = (1.0 + amount * 0.012).clamp(0.02, 50.0);
                        let prev = (1.0 + applied * 0.012).clamp(0.02, 50.0);
                        if (factor - prev).abs() < 1.0e-5 {
                            return;
                        }
                        transform::scale_matrix(axis, factor / prev, centre)
                    }
                };
                if self.doc.scene.transform_selection(matrix) > 0 {
                    self.drag = Drag::Gizmo {
                        axis,
                        mode,
                        start_screen,
                        centre,
                        applied: amount,
                    };
                    self.touch_document();
                }
            }
            Drag::MovePlane {
                start_screen,
                applied,
            } => {
                let delta_screen = local - start_screen;
                let raw = shapes::drag_world_delta(camera, delta_screen, size);
                let snapped = self
                    .settings
                    .snap
                    .translation(raw, self.doc.render.grid_spacing.max(1.0));
                let delta = snapped - applied;
                if delta.length_squared() < 1.0e-8 {
                    return;
                }
                if self.doc.scene.translate_selection(delta) > 0 {
                    self.drag = Drag::MovePlane {
                        start_screen,
                        applied: snapped,
                    };
                    self.touch_document();
                }
            }
            _ => {}
        }
    }

    fn finish_edit_tool(&mut self) {
        match &self.drag {
            Drag::Stroke { .. } => self.finish_stroke(),
            Drag::Erase => {
                let changed = self
                    .gesture_before
                    .as_ref()
                    .map(|before| before.scene.curves.len() != self.doc.scene.curves.len())
                    .unwrap_or(false);
                self.commit_gesture("Erase", changed);
            }
            Drag::Liquify { .. } => {
                let changed = self
                    .gesture_before
                    .as_ref()
                    .map(|before| before.scene.revision != self.doc.scene.revision)
                    .unwrap_or(false);
                self.commit_gesture("Liquify", changed);
            }
            Drag::Gizmo { mode, .. } => {
                let label = match mode {
                    TransformMode::Translate => "Move selection",
                    TransformMode::Rotate => "Rotate selection",
                    TransformMode::Scale => "Scale selection",
                };
                self.commit_gesture(label, true);
            }
            Drag::MovePlane { applied, .. } => {
                self.commit_gesture("Move selection", applied.length_squared() > 1.0e-8)
            }
            Drag::Shape { .. } => self.finish_shape_drag(),
            _ => {
                self.drag = Drag::None;
                self.gesture_before = None;
            }
        }
    }

    fn begin_shape(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let plane = self.drawing_plane(camera);
        let mut guides = std::mem::take(&mut self.doc.scene.guides);
        let point = capture::project_to_surface(local, camera, size, &mut guides, plane);
        self.doc.scene.guides = guides;
        let Some(point) = point else {
            self.cancel_gesture();
            return;
        };
        let id = self.doc.scene.new_curve_id();
        let brush = self.current_brush();
        let normal = point.normal.normalize_or_zero();
        let normal = if normal.length_squared() < 1.0e-6 {
            camera.forward()
        } else {
            normal
        };
        let curve = match self.tool {
            Tool::Line => {
                shapes::make_line(id, point.position, point.position, normal, &brush, false)
            }
            Tool::Circle => shapes::make_circle(id, point.position, point.position, normal, &brush),
            Tool::Ellipse => {
                shapes::make_ellipse_drag(id, point.position, point.position, normal, &brush, 0.7)
            }
            Tool::Polygon => shapes::make_polygon(
                id,
                point.position,
                0.5,
                self.settings.shape_sides,
                normal,
                0.0,
                &brush,
            ),
            _ => return,
        };
        let id = self.doc.scene.add_curve(curve);
        self.drag = Drag::Shape {
            id,
            start: point.position,
            normal,
            start_screen: local,
            moved: false,
        };
        self.needs_render = true;
    }

    fn update_shape(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let (id, start, normal, start_screen) = match self.drag.clone() {
            Drag::Shape {
                id,
                start,
                normal,
                start_screen,
                ..
            } => (id, start, normal, start_screen),
            _ => return,
        };
        if local.distance(start_screen) < 2.0 {
            return;
        }
        let plane = self.drawing_plane(camera);
        let mut guides = std::mem::take(&mut self.doc.scene.guides);
        let point = capture::project_to_surface(local, camera, size, &mut guides, plane);
        self.doc.scene.guides = guides;
        let Some(point) = point else { return };
        let brush = self.current_brush();
        let shift = false;
        let curve = match self.tool {
            Tool::Line => shapes::make_line(id, start, point.position, normal, &brush, shift),
            Tool::Circle => {
                let radius = (point.position - start).length().max(0.5);
                shapes::make_circle(
                    id,
                    start,
                    start + normal.cross(camera.forward()).normalize_or_zero() * radius,
                    normal,
                    &brush,
                )
            }
            Tool::Ellipse => {
                shapes::make_ellipse_drag(id, start, point.position, normal, &brush, 0.72)
            }
            Tool::Polygon => {
                let radius = (point.position - start).length().max(0.5);
                shapes::make_polygon(
                    id,
                    start,
                    radius,
                    self.settings.shape_sides,
                    normal,
                    0.0,
                    &brush,
                )
            }
            _ => return,
        };
        if let Some(existing) = self.doc.scene.curve_mut(id) {
            *existing = curve;
        }
        self.drag = Drag::Shape {
            id,
            start,
            normal,
            start_screen,
            moved: true,
        };
        self.needs_render = true;
    }

    fn finish_shape(&mut self, _camera: &Camera, _size: Vec2, _local: Vec2) {
        self.finish_shape_drag();
    }

    fn finish_shape_drag(&mut self) {
        let (id, moved) = match self.drag.clone() {
            Drag::Shape { id, moved, .. } => (id, moved),
            _ => return,
        };
        if !moved {
            self.doc.scene.remove_curve(id);
            self.drag = Drag::None;
            self.gesture_before = None;
            self.needs_render = true;
            return;
        }
        if let Some(curve) = self.doc.scene.curve(id).cloned() {
            self.doc.scene.selection.only_curve(id);
            self.doc.scene.mirror_curve_if_enabled(&curve);
        }
        self.doc.scene.selection.only_curve(id);
        self.commit_gesture("Draw shape", true);
    }

    fn place_guide_at(&mut self, camera: &Camera, size: Vec2, local: Vec2) {
        let plane = self.drawing_plane(camera);
        let mut guides = std::mem::take(&mut self.doc.scene.guides);
        let point = capture::project_to_surface(local, camera, size, &mut guides, plane);
        self.doc.scene.guides = guides;
        let Some(point) = point else {
            self.cancel_gesture();
            return;
        };
        let normal = if point.normal.length_squared() > 1.0e-6 {
            point.normal
        } else {
            camera.forward()
        };
        self.place_plane_at(point.position, normal);
        self.cancel_gesture();
    }

    // -- misc ---------------------------------------------------------------

    fn drawing_plane(&mut self, camera: &Camera) -> (Vec3, Vec3) {
        let through = self
            .doc
            .scene
            .selection_centre()
            .or_else(|| {
                self.doc
                    .scene
                    .active_guide
                    .and_then(|id| self.doc.scene.guide(id))
                    .map(|g| g.transform.w_axis.truncate())
            })
            .unwrap_or(camera.target);
        camera.drawing_plane(Some(through), 0.0)
    }
}

// ---------------------------------------------------------------------------
// Media capture worker
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_media_job(
    doc: Document,
    path: PathBuf,
    format: ExportFormat,
    width: u32,
    height: u32,
    quality: MeshQuality,
    frames: u32,
    fps: u32,
    delay_ms: u32,
    tx: std::sync::mpsc::Sender<MediaMessage>,
) -> Result<String> {
    use image::RgbaImage;

    let _ = tx.send(MediaMessage::Progress(0.0, "Creating render device".into()));
    let (device, queue) = gpu::create_headless_device()?;
    let mut renderer = GpuRenderer::new(device, queue, [width.max(1), height.max(1)]);
    let base = doc.camera.clone();
    let duration = doc.animation.effective_duration();
    let mut render_frame = |index: u32| -> Result<RgbaImage> {
        let time = if frames <= 1 {
            0.0
        } else {
            index as f32 / fps.max(1) as f32
        };
        let mut frame_doc = doc.clone();
        frame_doc.camera = doc.animation.camera_at(&base, time.min(duration.max(0.1)));
        renderer.render(&frame_doc, [width.max(1), height.max(1)], quality, time);
        renderer.capture_rgba(width.max(1), height.max(1))
    };

    let result = match format {
        ExportFormat::Png => {
            let image = render_frame(0)?;
            let _ = tx.send(MediaMessage::Progress(0.9, "Writing PNG".into()));
            io::export_png(&image, &path)?;
            Ok("PNG".to_string())
        }
        ExportFormat::Gif => {
            let mut images = Vec::with_capacity(frames as usize);
            for index in 0..frames {
                images.push(render_frame(index)?);
                let p = if frames > 0 {
                    (index as f32 + 1.0) / frames as f32 * 0.92
                } else {
                    0.92
                };
                let _ = tx.send(MediaMessage::Progress(
                    p,
                    format!("Rendering GIF frame {}/{}", index + 1, frames),
                ));
            }
            let _ = tx.send(MediaMessage::Progress(0.95, "Encoding GIF".into()));
            io::export_gif(&images, &path, delay_ms)?;
            Ok(format!("GIF · {} frames", frames))
        }
        ExportFormat::Mp4 => {
            let _ = tx.send(MediaMessage::Progress(0.02, "Rendering MP4 frames".into()));
            let progress_tx = tx.clone();
            io::export_mp4_stream(&path, frames, fps, |index| {
                let p = if frames > 0 {
                    (index as f32 + 1.0) / frames as f32 * 0.95
                } else {
                    0.95
                };
                let _ = progress_tx.send(MediaMessage::Progress(
                    p,
                    format!("Rendering frame {}/{}", index + 1, frames),
                ));
                render_frame(index)
                    .map_err(|error| plume_core::PlumeError::Invalid(format!("{error:#}")))
            })?;
            Ok(format!("MP4 · {} frames", frames))
        }
        _ => unreachable!(),
    };
    result
}

fn rotation_from_y(normal: Vec3) -> Mat4 {
    let normal = normal.normalize_or_zero();
    if normal.length_squared() < 1.0e-8 {
        return Mat4::IDENTITY;
    }
    let axis = Vec3::Y.cross(normal);
    if axis.length_squared() < 1.0e-8 {
        if normal.y < 0.0 {
            Mat4::from_rotation_x(std::f32::consts::PI)
        } else {
            Mat4::IDENTITY
        }
    } else {
        Mat4::from_quat(plume_core::Quat::from_axis_angle(
            axis.normalize(),
            Vec3::Y.dot(normal).clamp(-1.0, 1.0).acos(),
        ))
    }
}

fn human_bytes(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

// Fields shared with `ui.rs` but declared here to keep the state in one place.
impl PlumeApp {
    pub fn update_animation(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let dt = (now - self.last_time).clamp(0.0, 0.12) as f32;
        self.last_time = now;
        if self.doc.animation.playing {
            if self.doc.animation.tick(dt) {
                self.preview_time = Some(self.doc.animation.time);
                self.needs_render = true;
                ctx.request_repaint_after(Duration::from_millis(16));
            }
            if !self.doc.animation.playing {
                self.preview_time = Some(self.doc.animation.time);
            }
        }
    }

    pub fn install_context(&mut self, ctx: &egui::Context) {
        self.last_ctx = Some(ctx.clone());
        let title = format!(
            "PLUME — {}{}",
            self.doc.name,
            if self.dirty { " •" } else { "" }
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_ignores_view_state_but_tracks_content() {
        let mut document = Document::new("Baseline");
        let saved = document_fingerprint(&document);

        document.modified = "later".into();
        document.camera.position.x += 12.0;
        document.scene.selection.curves.insert(42);
        document.thumbnail = Some(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ));
        assert_eq!(saved, document_fingerprint(&document));

        document.name = "Renamed".into();
        assert_ne!(saved, document_fingerprint(&document));
    }

    #[test]
    fn undoing_back_to_the_saved_state_clears_the_dirty_marker() {
        let mut document = Document::new("Baseline");
        let saved = document_fingerprint(&document);
        let mut history = History::new(8);

        history.record("Rename", &document);
        document.name = "Changed".into();
        assert!(differs_from_baseline(Some(&saved), &document));

        history.undo(&mut document);
        assert!(!differs_from_baseline(Some(&saved), &document));

        history.redo(&mut document);
        assert!(differs_from_baseline(Some(&saved), &document));
    }
}
