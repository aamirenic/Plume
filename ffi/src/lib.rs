//! UniFFI bindings for PLUME's mobile shells.
//!
//! The bridge deliberately exposes document-level operations rather than a
//! second editor implementation.  Android and iOS clients load a gallery
//! scene or a serialized document, navigate with the same camera routines as
//! the desktop app, draw strokes through the shared capture pipeline, and ask
//! the host for PNG previews or model exports.
//!
//! Generate Kotlin bindings after building the library:
//!
//! ```text
//! cargo run -p plume-ffi --bin uniffi-bindgen -- generate \
//!   --library target/debug/plume_ffi.dll --language kotlin \
//!   --out-dir android/app/src/main/java
//! ```
//!
//! On Android use the `.so` produced for the device ABI instead of the DLL.

use std::io::Cursor;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use plume_core::brush::BrushLibrary;
use plume_core::capture::{self, StrokeConfig};
use plume_core::gallery;
use plume_core::io::{self, ExportOptions};
use plume_core::scene::{Document, History, MeshQuality};
use plume_core::selection::{self, PickSettings};
use plume_core::viewport::Camera;
use plume_core::{Vec2, Vec3};

uniffi::setup_scaffolding!();

/// Errors surfaced to the host language as typed exceptions.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{reason}")]
    Message { reason: String },
}

impl BridgeError {
    fn message(message: impl Into<String>) -> Self {
        Self::Message {
            reason: message.into(),
        }
    }
}

/// A gallery scene exposed to the mobile scene browser.
#[derive(Clone, Debug, uniffi::Record)]
pub struct GalleryEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
}

/// A brush exposed to the mobile tool rail.
#[derive(Clone, Debug, uniffi::Record)]
pub struct BrushEntry {
    pub id: String,
    pub name: String,
    pub category: String,
    pub size: f32,
}

/// Lightweight document statistics used by the mobile status bar.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DocumentInfo {
    pub name: String,
    pub curves: u32,
    pub guides: u32,
    pub groups: u32,
    pub control_points: u32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub brush_id: String,
    pub camera_distance: f32,
    pub dirty: bool,
}

/// Result of writing a model or a PLUME package.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ExportSummary {
    pub path: String,
    pub bytes: u64,
    pub objects: u32,
    pub triangles: u32,
}

/// One mesh in a compact, GPU-ready binary layout for the Android AR preview.
///
/// `positions` and `normals` are little-endian `f32` triples; `indices` is a
/// little-endian `u32` triangle list.  Packing avoids one JNI object per
/// component and keeps bridge overhead low on large scenes.
#[derive(Clone, Debug, uniffi::Record)]
pub struct SceneMesh {
    pub name: String,
    pub positions: Vec<u8>,
    pub normals: Vec<u8>,
    pub indices: Vec<u8>,
    pub vertex_count: u32,
    pub triangle_count: u32,
    /// Linear RGBA in 0..1.
    pub color: Vec<f32>,
    pub emission: f32,
}

struct SessionState {
    doc: Document,
    history: History,
    library: BrushLibrary,
    brush_id: String,
    viewport: Vec2,
    raw: Vec<(Vec3, f32, f64)>,
    stroke_before: Option<Document>,
    playhead_base: Option<Camera>,
    dirty: bool,
}

impl SessionState {
    fn new() -> Self {
        let doc = Document::new("Untitled");
        let mut library = BrushLibrary::default();
        let brush_id = if library.get("sketch-medium").is_some() {
            "sketch-medium".to_string()
        } else {
            library
                .brushes
                .first()
                .map(|brush| brush.id.clone())
                .unwrap_or_default()
        };
        let known: std::collections::BTreeSet<&str> = library
            .brushes
            .iter()
            .map(|brush| brush.id.as_str())
            .collect();
        library.favorites.retain(|id| known.contains(id.as_str()));
        Self {
            doc,
            history: History::new(100),
            library,
            brush_id,
            viewport: Vec2::new(800.0, 600.0),
            raw: Vec::new(),
            stroke_before: None,
            playhead_base: None,
            dirty: false,
        }
    }

    fn info(&self) -> DocumentInfo {
        let stats = self.doc.stats();
        DocumentInfo {
            name: self.doc.name.clone(),
            curves: stats.curves.min(u32::MAX as usize) as u32,
            guides: stats.guides.min(u32::MAX as usize) as u32,
            groups: stats.groups.min(u32::MAX as usize) as u32,
            control_points: stats.control_points.min(u32::MAX as usize) as u32,
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            brush_id: self.brush_id.clone(),
            camera_distance: self.doc.camera.distance(),
            dirty: self.dirty,
        }
    }

    fn brush(&self) -> plume_core::brush::Brush {
        self.library
            .get(&self.brush_id)
            .cloned()
            .or_else(|| self.library.brushes.first().cloned())
            .unwrap_or_default()
    }

    fn add_sample(&mut self, x: f32, y: f32, pressure: f32) {
        let camera = self.doc.camera.clone();
        let plane = camera.drawing_plane(None, 0.0);
        let screen = Vec2::new(x, y);
        if let Some(surface) = capture::project_to_surface(
            screen,
            &camera,
            self.viewport,
            &mut self.doc.scene.guides,
            plane,
        ) {
            let timestamp = self.raw.len() as f64 * 0.008;
            self.raw
                .push((surface.position, pressure.clamp(0.01, 1.0), timestamp));
        }
    }
}

/// A mobile editing session.  It is safe to call from any thread and keeps
/// one document plus its undo stack alive for the lifetime of the screen.
#[derive(uniffi::Object)]
pub struct PlumeSession {
    state: Mutex<SessionState>,
}

impl PlumeSession {
    fn lock(&self) -> Result<MutexGuard<'_, SessionState>, BridgeError> {
        self.state
            .lock()
            .map_err(|_| BridgeError::message("the PLUME session lock was poisoned"))
    }
}

#[uniffi::export]
impl PlumeSession {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(SessionState::new()),
        })
    }

    /// Every built-in gallery scene, in display order.
    pub fn gallery(&self) -> Vec<GalleryEntry> {
        gallery::GALLERY
            .iter()
            .map(|item| GalleryEntry {
                id: item.id.to_string(),
                name: item.name.to_string(),
                description: item.description.to_string(),
                category: item.category.to_string(),
            })
            .collect()
    }

    /// Every built-in brush.  The desktop and mobile editors share this list.
    pub fn brushes(&self) -> Result<Vec<BrushEntry>, BridgeError> {
        let state = self.lock()?;
        Ok(state
            .library
            .brushes
            .iter()
            .map(|brush| BrushEntry {
                id: brush.id.clone(),
                name: brush.name.clone(),
                category: brush.category.label().to_string(),
                size: brush.base_size,
            })
            .collect())
    }

    pub fn set_brush(&self, id: String) -> Result<bool, BridgeError> {
        let mut state = self.lock()?;
        if state.library.get(&id).is_some() {
            state.brush_id = id;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Load one of the generated gallery scenes, discarding undo history.
    pub fn load_gallery(&self, id: String) -> Result<DocumentInfo, BridgeError> {
        let doc = gallery::build_document(&id)
            .ok_or_else(|| BridgeError::message(format!("unknown PLUME gallery scene \"{id}\"")))?;
        let mut state = self.lock()?;
        state.doc = doc;
        state.history.clear();
        state.raw.clear();
        state.stroke_before = None;
        state.playhead_base = None;
        state.dirty = false;
        Ok(state.info())
    }

    /// Replace the document with a blank scene.
    pub fn new_document(&self) -> Result<DocumentInfo, BridgeError> {
        let mut state = self.lock()?;
        state.doc = Document::new("Untitled");
        state.history.clear();
        state.raw.clear();
        state.stroke_before = None;
        state.playhead_base = None;
        state.dirty = false;
        Ok(state.info())
    }

    pub fn document_info(&self) -> Result<DocumentInfo, BridgeError> {
        Ok(self.lock()?.info())
    }

    /// The complete document as UTF-8 JSON.  This is the same schema used by
    /// the `.plume` package and is suitable for autosave on the device.
    pub fn document_json(&self) -> Result<String, BridgeError> {
        let state = self.lock()?;
        serde_json::to_string(&state.doc)
            .map_err(|error| BridgeError::message(format!("could not serialize document: {error}")))
    }

    pub fn load_document_json(&self, json: String) -> Result<DocumentInfo, BridgeError> {
        let doc: Document = serde_json::from_str(&json).map_err(|error| {
            BridgeError::message(format!("could not parse PLUME document: {error}"))
        })?;
        let mut state = self.lock()?;
        state.doc = doc;
        state.history.clear();
        state.raw.clear();
        state.stroke_before = None;
        state.playhead_base = None;
        state.dirty = false;
        Ok(state.info())
    }

    /// The mobile viewport size in points.  Call whenever the canvas resizes.
    pub fn set_viewport(&self, width: f32, height: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        state.viewport = Vec2::new(width.max(16.0), height.max(16.0));
        Ok(())
    }

    pub fn orbit(&self, dx: f32, dy: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        state.doc.camera.orbit_pixels(dx, dy);
        state.dirty = true;
        Ok(())
    }

    pub fn pan(&self, dx: f32, dy: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        let viewport = state.viewport;
        let camera = &mut state.doc.camera;
        camera.pan_pixels(dx, dy, viewport);
        state.dirty = true;
        Ok(())
    }

    pub fn zoom(&self, factor: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        state.doc.camera.zoom(factor);
        state.dirty = true;
        Ok(())
    }

    /// Fit all visible geometry and guides into the current viewport.
    pub fn frame_scene(&self) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        let (min, max) = state.doc.scene.bounds_visible();
        let aspect = state.viewport.x / state.viewport.y.max(1.0);
        state.doc.camera.frame_bounds(min, max, aspect);
        Ok(())
    }

    /// Camera-aware software preview as PNG bytes.  Use this for the live
    /// editor canvas; the document camera is preserved.
    pub fn preview_png(&self, width: u32, height: u32) -> Result<Vec<u8>, BridgeError> {
        let state = self.lock()?;
        let image = gallery::preview(&state.doc, width.clamp(64, 1024), height.clamp(48, 1024));
        png_bytes(image)
    }

    /// Auto-framed still for the scene browser.  Does not change the session.
    pub fn gallery_preview_png(
        &self,
        id: String,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, BridgeError> {
        let doc = gallery::build_document(&id)
            .ok_or_else(|| BridgeError::message(format!("unknown PLUME gallery scene \"{id}\"")))?;
        let image = gallery::thumbnail(&doc, width.clamp(64, 1024), height.clamp(48, 1024));
        png_bytes(image)
    }

    /// Erase the closest visible curve under a viewport point.  Returns true
    /// when the scene changed.
    pub fn erase_at(&self, x: f32, y: f32) -> Result<bool, BridgeError> {
        let mut state = self.lock()?;
        let settings = PickSettings {
            tolerance: 16.0,
            ..PickSettings::default()
        };
        let picked = selection::pick(
            &state.doc.scene,
            &state.doc.camera,
            state.viewport,
            Vec2::new(x, y),
            &settings,
        );
        let Some(id) = picked.curve else {
            return Ok(false);
        };
        let before = state.doc.clone();
        if state.doc.scene.remove_curve(id).is_none() {
            return Ok(false);
        }
        state.doc.scene.touch();
        state.doc.modified = plume_core::io::plume::utc_timestamp();
        state.history.push_snapshot("Erase", before);
        state.playhead_base = None;
        state.dirty = true;
        Ok(true)
    }

    /// Remove every selected curve.  Returns the number deleted.
    pub fn delete_selection(&self) -> Result<u32, BridgeError> {
        let mut state = self.lock()?;
        let ids: Vec<plume_core::CurveId> =
            state.doc.scene.selection.curves.iter().copied().collect();
        if ids.is_empty() {
            return Ok(0);
        }
        let before = state.doc.clone();
        let mut removed = 0u32;
        for id in ids {
            if state.doc.scene.remove_curve(id).is_some() {
                removed += 1;
            }
        }
        if removed > 0 {
            state.doc.scene.touch();
            state.doc.modified = plume_core::io::plume::utc_timestamp();
            state.history.push_snapshot("Delete", before);
            state.playhead_base = None;
            state.dirty = true;
        }
        Ok(removed)
    }

    /// Total timeline length in seconds, including shots and keyframes.
    pub fn animation_duration(&self) -> Result<f64, BridgeError> {
        Ok(self.lock()?.doc.animation.effective_duration() as f64)
    }

    /// Scrub the camera animation without losing the authoring camera.  Call
    /// [`Self::stop_playhead`] to return to the camera the user was editing.
    pub fn set_playhead(&self, seconds: f64) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        let current = state.doc.camera.clone();
        let base = state.playhead_base.get_or_insert(current).clone();
        state.doc.camera = state
            .doc
            .animation
            .camera_at(&base, seconds.max(0.0) as f32);
        Ok(())
    }

    pub fn stop_playhead(&self) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        if let Some(base) = state.playhead_base.take() {
            state.doc.camera = base;
        }
        Ok(())
    }

    /// Upload-ready geometry for the AR preview, in document millimetres.
    /// The Android renderer fits and scales the combined bounds to a tabletop
    /// model, so no unit conversion happens here.
    pub fn scene_meshes(&self, quality: String) -> Result<Vec<SceneMesh>, BridgeError> {
        let state = self.lock()?;
        let options = ExportOptions {
            quality: quality_from_str(&quality),
            include_guides: false,
            include_hidden: false,
            unit_scale: 1.0,
            z_up: false,
            merge: false,
            double_sided: true,
        };
        let meshes = io::collect_export_meshes(&state.doc, &options);
        Ok(meshes
            .into_iter()
            .filter(|item| !item.mesh.is_empty())
            .map(|item| SceneMesh {
                name: item.name,
                positions: pack_f32(item.mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z])),
                normals: pack_f32(item.mesh.normals.iter().flat_map(|n| [n.x, n.y, n.z])),
                indices: pack_u32(item.mesh.indices.iter().copied()),
                vertex_count: item.mesh.positions.len().min(u32::MAX as usize) as u32,
                triangle_count: (item.mesh.indices.len() / 3).min(u32::MAX as usize) as u32,
                color: vec![item.color.x, item.color.y, item.color.z, item.color.w],
                emission: item.emission,
            })
            .collect())
    }

    pub fn begin_stroke(&self, x: f32, y: f32, pressure: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        state.raw.clear();
        state.stroke_before = Some(state.doc.clone());
        state.add_sample(x, y, pressure);
        Ok(())
    }

    pub fn extend_stroke(&self, x: f32, y: f32, pressure: f32) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        if state.stroke_before.is_none() {
            return Ok(());
        }
        state.add_sample(x, y, pressure);
        Ok(())
    }

    /// Finalize the current stroke.  Returns `true` when a curve was added.
    pub fn end_stroke(&self) -> Result<bool, BridgeError> {
        let mut state = self.lock()?;
        let Some(before) = state.stroke_before.take() else {
            return Ok(false);
        };
        if state.raw.len() < 2 {
            state.raw.clear();
            return Ok(false);
        }
        let id = state.doc.scene.new_curve_id();
        let brush = state.brush();
        let config = StrokeConfig::default();
        let Some(curve) = capture::build_curve_from_world(id, &state.raw, &brush, &config) else {
            state.raw.clear();
            return Ok(false);
        };
        let curve_id = state.doc.scene.add_curve(curve);
        state.doc.scene.selection.curves.clear();
        state.doc.scene.selection.curves.insert(curve_id);
        state.doc.scene.touch();
        state.doc.modified = plume_core::io::plume::utc_timestamp();
        state.history.push_snapshot("Stroke", before);
        state.raw.clear();
        state.dirty = true;
        Ok(true)
    }

    pub fn cancel_stroke(&self) -> Result<(), BridgeError> {
        let mut state = self.lock()?;
        state.raw.clear();
        state.stroke_before = None;
        Ok(())
    }

    pub fn undo(&self) -> Result<bool, BridgeError> {
        let mut state = self.lock()?;
        let SessionState { history, doc, .. } = &mut *state;
        let changed = history.undo(doc).is_some();
        if changed {
            state.dirty = true;
        }
        Ok(changed)
    }

    pub fn redo(&self) -> Result<bool, BridgeError> {
        let mut state = self.lock()?;
        let SessionState { history, doc, .. } = &mut *state;
        let changed = history.redo(doc).is_some();
        if changed {
            state.dirty = true;
        }
        Ok(changed)
    }

    /// A software-projected PNG preview.  No GPU is required on the device.
    pub fn thumbnail_png(&self, width: u32, height: u32) -> Result<Vec<u8>, BridgeError> {
        let state = self.lock()?;
        let thumbnail =
            gallery::thumbnail(&state.doc, width.clamp(64, 1024), height.clamp(48, 1024));
        png_bytes(thumbnail)
    }

    /// Write a document (`plume`, `feather`) or a mesh (`obj`, `glb`, `gltf`,
    /// `stl`, `usdz`).  The parent directory must already exist.
    pub fn export_document(
        &self,
        path: String,
        format: String,
        quality: String,
    ) -> Result<ExportSummary, BridgeError> {
        let state = self.lock()?;
        let path = std::path::PathBuf::from(path);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    BridgeError::message(format!(
                        "could not create export folder {}: {error}",
                        parent.display()
                    ))
                })?;
            }
        }
        let quality = quality_from_str(&quality);
        let options = ExportOptions {
            quality,
            ..ExportOptions::default()
        };
        let format = format.to_ascii_lowercase();
        let (objects, triangles) = match format.as_str() {
            "plume" => {
                let thumb = gallery::thumbnail(&state.doc, 512, 320);
                io::save_plume(&path, &state.doc, Some(&thumb), &Default::default())
                    .map_err(bridge_error)?;
                (state.doc.scene.curves.len(), 0)
            }
            "feather" => {
                let thumb = gallery::thumbnail(&state.doc, 512, 320);
                io::export_feather(&state.doc, &path, Some(&thumb)).map_err(bridge_error)?;
                (state.doc.scene.curves.len(), 0)
            }
            "obj" => {
                let report = io::export_obj(&state.doc, &path, &options).map_err(bridge_error)?;
                (report.objects, report.triangles)
            }
            "glb" => {
                let report = io::export_glb(&state.doc, &path, &options).map_err(bridge_error)?;
                (report.objects, report.triangles)
            }
            "gltf" => {
                let report = io::export_gltf(&state.doc, &path, &options).map_err(bridge_error)?;
                (report.objects, report.triangles)
            }
            "stl" => {
                let report =
                    io::export_stl(&state.doc, &path, &options, true).map_err(bridge_error)?;
                (report.objects, report.triangles)
            }
            "usdz" => {
                let report = io::export_usdz(&state.doc, &path, &options).map_err(bridge_error)?;
                (report.objects, report.triangles)
            }
            other => {
                return Err(BridgeError::message(format!(
                    "unsupported export format \"{other}\""
                )))
            }
        };
        let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        Ok(ExportSummary {
            path: pretty_path(&path),
            bytes,
            objects: objects.min(u32::MAX as usize) as u32,
            triangles: triangles.min(u32::MAX as usize) as u32,
        })
    }
}

fn quality_from_str(value: &str) -> MeshQuality {
    match value.to_ascii_lowercase().as_str() {
        "draft" | "low" => MeshQuality::Draft,
        "export" | "high" => MeshQuality::Export,
        _ => MeshQuality::Normal,
    }
}

fn png_bytes(image: image::RgbaImage) -> Result<Vec<u8>, BridgeError> {
    let mut cursor = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|error| BridgeError::message(format!("could not encode preview: {error}")))?;
    Ok(cursor.into_inner())
}

fn pack_f32(values: impl Iterator<Item = f32>) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn pack_u32(values: impl Iterator<Item = u32>) -> Vec<u8> {
    let mut out = Vec::new();
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn bridge_error(error: plume_core::PlumeError) -> BridgeError {
    BridgeError::message(error.to_string())
}

fn pretty_path(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_exposes_scenes_and_brushes() {
        let session = PlumeSession::new();
        let scenes = session.gallery();
        assert!(scenes.iter().any(|scene| scene.id == "ribbon-bloom"));
        let brushes = session.brushes().expect("brushes");
        assert!(brushes.len() >= 30);
        assert!(session.set_brush("ink-fine".into()).expect("set brush"));
        assert!(!session.set_brush("missing".into()).expect("set brush"));
    }

    #[test]
    fn strokes_round_trip_through_undo() {
        let session = PlumeSession::new();
        session.set_viewport(900.0, 640.0).expect("viewport");
        session.begin_stroke(360.0, 330.0, 0.5).expect("begin");
        session.extend_stroke(430.0, 300.0, 0.6).expect("extend");
        session.extend_stroke(500.0, 350.0, 0.7).expect("extend");
        assert!(session.end_stroke().expect("end"));
        assert!(session.document_info().expect("info").curves >= 1);
        assert!(session.undo().expect("undo"));
        assert_eq!(session.document_info().expect("info").curves, 0);
        assert!(session.redo().expect("redo"));
        assert!(session.document_info().expect("info").curves >= 1);
    }

    #[test]
    fn preview_is_a_png() {
        let session = PlumeSession::new();
        session
            .load_gallery("ribbon-bloom".into())
            .expect("gallery");
        let png = session.thumbnail_png(320, 200).expect("thumbnail");
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn exports_a_mesh() {
        let session = PlumeSession::new();
        session
            .load_gallery("ribbon-bloom".into())
            .expect("gallery");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ribbon.glb");
        let summary = session
            .export_document(
                path.to_string_lossy().to_string(),
                "glb".into(),
                "normal".into(),
            )
            .expect("export");
        assert!(summary.bytes > 0);
        assert!(path.is_file());
    }

    #[test]
    fn preview_tracks_orbit_and_stays_a_png() {
        let session = PlumeSession::new();
        session
            .load_gallery("ribbon-bloom".into())
            .expect("gallery");
        session.set_viewport(600.0, 400.0).expect("viewport");
        let before = session.preview_png(320, 200).expect("preview");
        assert!(before.starts_with(&[0x89, b'P', b'N', b'G']));
        session.orbit(90.0, 20.0).expect("orbit");
        let after = session.preview_png(320, 200).expect("preview");
        assert_ne!(before, after);
        let still = session
            .gallery_preview_png("wire-flora".into(), 320, 200)
            .expect("gallery preview");
        assert!(still.starts_with(&[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn scene_meshes_are_packed_and_consistent() {
        let session = PlumeSession::new();
        session
            .load_gallery("ribbon-bloom".into())
            .expect("gallery");
        let meshes = session.scene_meshes("normal".into()).expect("meshes");
        assert!(!meshes.is_empty());
        for mesh in meshes {
            assert_eq!(mesh.positions.len() as u32, mesh.vertex_count * 12);
            assert_eq!(mesh.normals.len(), mesh.positions.len());
            assert_eq!(mesh.indices.len() as u32, mesh.triangle_count * 12);
            assert_eq!(mesh.color.len(), 4);
        }
    }

    #[test]
    fn eraser_picks_and_removes_a_curve() {
        let session = PlumeSession::new();
        session.set_viewport(800.0, 600.0).expect("viewport");
        session.begin_stroke(360.0, 330.0, 0.6).expect("begin");
        session.extend_stroke(440.0, 300.0, 0.6).expect("extend");
        session.extend_stroke(500.0, 340.0, 0.6).expect("extend");
        assert!(session.end_stroke().expect("end"));
        assert_eq!(session.document_info().expect("info").curves, 1);
        // Probe along the finished stroke; the exact midpoint depends on
        // projection, so try several points inside a generous tolerance.
        let mut erased = false;
        for (x, y) in [
            (360.0, 330.0),
            (400.0, 315.0),
            (440.0, 300.0),
            (500.0, 340.0),
        ] {
            if session.erase_at(x, y).expect("erase") {
                erased = true;
                break;
            }
        }
        assert!(erased);
        assert_eq!(session.document_info().expect("info").curves, 0);
        assert!(session.undo().expect("undo"));
        assert_eq!(session.document_info().expect("info").curves, 1);
    }

    #[test]
    fn animation_playhead_restores_the_authoring_camera() {
        let session = PlumeSession::new();
        session
            .load_gallery("ribbon-bloom".into())
            .expect("gallery");
        let base = session.document_info().expect("info").camera_distance;
        assert!(session.animation_duration().expect("duration") > 0.0);
        session.set_playhead(0.5).expect("playhead");
        session.stop_playhead().expect("stop");
        let restored = session.document_info().expect("info").camera_distance;
        assert!((base - restored).abs() < 1.0e-3);
    }
}
