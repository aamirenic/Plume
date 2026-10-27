//! Import and export.
//!
//! All writers are implemented from the published format specifications. No
//! third-party application assets are required, and every format can be
//! produced on a machine without network access (MP4 uses a local ffmpeg).

pub mod feather;
pub mod gltf_io;
pub mod media;
pub mod obj;
pub mod plume;
pub mod stl;
pub mod usdz;

use crate::brush::Brush;
use crate::guide::build_guide_mesh;
use crate::mesh::Mesh;
use crate::scene::{curve_mesh, Document, MeshQuality};
use crate::{PlumeError, Result, Vec4};
use std::path::{Path, PathBuf};

pub use feather::{export_feather, import_feather};
pub use gltf_io::{export_glb, export_gltf, import_gltf};
pub use media::{export_gif, export_mp4, export_mp4_stream, export_png};
pub use obj::{export_obj, import_obj};
pub use plume::{load_plume, plume_bytes, save_plume, PlumePackage};
pub use stl::{export_stl, import_stl};
pub use usdz::export_usdz;

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub quality: MeshQuality,
    /// Include visible guide surfaces as geometry.
    pub include_guides: bool,
    /// Include hidden curves (off by default).
    pub include_hidden: bool,
    /// Uniform scale applied on export. PLUME works in millimetres; OBJ, glTF
    /// and USD usually assume metres, so the default converts mm -> m.
    pub unit_scale: f32,
    /// Rotate Y-up to Z-up for DCC applications that expect it.
    pub z_up: bool,
    /// Merge everything into one mesh (STL has no object hierarchy).
    pub merge: bool,
    pub double_sided: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            quality: MeshQuality::Export,
            include_guides: false,
            include_hidden: false,
            unit_scale: 0.001,
            z_up: false,
            merge: false,
            double_sided: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportMesh {
    pub name: String,
    pub mesh: Mesh,
    pub color: Vec4,
    pub emission: f32,
}

impl ExportMesh {
    pub fn new(name: impl Into<String>, mesh: Mesh, color: Vec4) -> Self {
        Self {
            name: name.into(),
            mesh,
            color,
            emission: 0.0,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub path: PathBuf,
    pub objects: usize,
    pub vertices: usize,
    pub triangles: usize,
    pub bytes: u64,
}

pub fn collect_export_meshes(document: &Document, options: &ExportOptions) -> Vec<ExportMesh> {
    let scene = &document.scene;
    let mut out = Vec::new();
    for curve in &scene.curves {
        if !options.include_hidden && !scene.curve_visible(curve) {
            continue;
        }
        let mut mesh = curve_mesh(curve, options.quality);
        if mesh.is_empty() {
            continue;
        }
        apply_export_transform(&mut mesh, options);
        let name = if curve.name.trim().is_empty() {
            format!("curve_{}", curve.id)
        } else {
            curve.name.clone()
        };
        let mut em = ExportMesh::new(name, mesh, curve.color);
        em.emission = curve.brush.emission;
        out.push(em);
    }
    if options.include_guides {
        for guide in &scene.guides {
            if !guide.visible {
                continue;
            }
            let mut mesh = build_guide_mesh(&guide.kind);
            mesh.transform(guide.transform);
            if mesh.is_empty() {
                continue;
            }
            apply_export_transform(&mut mesh, options);
            out.push(ExportMesh::new(guide.name.clone(), mesh, guide.color));
        }
    }
    if options.merge && out.len() > 1 {
        let mut merged = Mesh::new();
        for m in &out {
            merged.append(&m.mesh);
        }
        let color = out
            .first()
            .map(|m| m.color)
            .unwrap_or(Vec4::new(0.85, 0.85, 0.85, 1.0));
        out = vec![ExportMesh::new("PLUME scene", merged, color)];
    }
    out
}

fn apply_export_transform(mesh: &mut Mesh, options: &ExportOptions) {
    let scale = options.unit_scale.max(1.0e-9);
    let mat = if options.z_up {
        crate::Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
    } else {
        crate::Mat4::IDENTITY
    };
    mesh.transform(crate::Mat4::from_scale(crate::Vec3::splat(scale)) * mat);
}

pub fn report_for(path: &Path, meshes: &[ExportMesh]) -> ExportReport {
    ExportReport {
        path: path.to_path_buf(),
        objects: meshes.len(),
        vertices: meshes.iter().map(|m| m.mesh.vertex_count()).sum(),
        triangles: meshes.iter().map(|m| m.mesh.triangle_count()).sum(),
        bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
    }
}

pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ' ') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.trim().is_empty() {
        "PLUME".into()
    } else {
        out.trim().to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ImportedScene {
    pub meshes: Vec<ImportMesh>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ImportMesh {
    pub name: String,
    pub mesh: Mesh,
    pub color: Vec4,
}

impl ImportedScene {
    pub fn to_document(&self, brush: &Brush, name: impl Into<String>) -> Result<Document> {
        if self.meshes.is_empty() {
            return Err(PlumeError::Invalid("import contains no geometry".into()));
        }
        let mut doc = Document::new(name);
        for imported in &self.meshes {
            let id = doc.scene.new_guide_id();
            let guide = crate::guide::Guide::imported(
                id,
                imported.name.clone(),
                imported.mesh.clone(),
                None,
            );
            doc.scene.add_guide(guide);
        }
        let _ = brush;
        Ok(doc)
    }
}
