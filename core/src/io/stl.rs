//! STL writer and reader (binary and ASCII).

use super::{
    collect_export_meshes, report_for, ExportOptions, ExportReport, ImportMesh, ImportedScene,
};
use crate::mesh::Mesh;
use crate::scene::Document;
use crate::{PlumeError, Result, Vec3, Vec4};
use std::io::Write;
use std::path::Path;

pub fn export_stl(
    document: &Document,
    path: &Path,
    options: &ExportOptions,
    binary: bool,
) -> Result<ExportReport> {
    let mut opts = options.clone();
    opts.merge = true;
    let meshes = collect_export_meshes(document, &opts);
    if meshes.is_empty() {
        return Err(PlumeError::Invalid("nothing to export".into()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if binary {
        write_binary(path, &meshes[0].mesh)?;
    } else {
        write_ascii(path, &meshes[0].mesh)?;
    }
    Ok(report_for(path, &meshes))
}

fn write_binary(path: &Path, mesh: &Mesh) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut w = std::io::BufWriter::new(file);
    let mut header = [0u8; 80];
    let text = format!("PLUME binary STL - {}", mesh.triangle_count());
    let bytes = text.as_bytes();
    header[..bytes.len().min(80)].copy_from_slice(&bytes[..bytes.len().min(80)]);
    w.write_all(&header)?;
    w.write_all(&(mesh.triangle_count() as u32).to_le_bytes())?;
    for tri in mesh.indices.as_chunks::<3>().0 {
        let a = mesh.positions[tri[0] as usize];
        let b = mesh.positions[tri[1] as usize];
        let c = mesh.positions[tri[2] as usize];
        let n = (b - a).cross(c - a).normalize_or_zero();
        for v in [n, a, b, c] {
            w.write_all(&v.x.to_le_bytes())?;
            w.write_all(&v.y.to_le_bytes())?;
            w.write_all(&v.z.to_le_bytes())?;
        }
        w.write_all(&0u16.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}

fn write_ascii(path: &Path, mesh: &Mesh) -> Result<()> {
    let mut out = String::with_capacity(mesh.triangle_count() * 180);
    out.push_str("solid PLUME\n");
    for tri in mesh.indices.as_chunks::<3>().0 {
        let a = mesh.positions[tri[0] as usize];
        let b = mesh.positions[tri[1] as usize];
        let c = mesh.positions[tri[2] as usize];
        let n = (b - a).cross(c - a).normalize_or_zero();
        out.push_str(&format!(
            "  facet normal {:.6} {:.6} {:.6}\n    outer loop\n      vertex {:.6} {:.6} {:.6}\n      vertex {:.6} {:.6} {:.6}\n      vertex {:.6} {:.6} {:.6}\n    endloop\n  endfacet\n",
            n.x, n.y, n.z, a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z
        ));
    }
    out.push_str("endsolid PLUME\n");
    std::fs::write(path, out)?;
    Ok(())
}

pub fn import_stl(path: &Path) -> Result<ImportedScene> {
    let data = std::fs::read(path)?;
    if data.len() >= 84 {
        let count = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
        if 84usize.saturating_add(count.saturating_mul(50)) == data.len() {
            return import_binary(&data, count);
        }
    }
    import_ascii(&data)
}

fn import_binary(data: &[u8], count: usize) -> Result<ImportedScene> {
    let mut mesh = Mesh::new();
    for i in 0..count {
        let base = 84 + i * 50;
        let mut vertices = [Vec3::ZERO; 3];
        for (j, v) in vertices.iter_mut().enumerate() {
            let off = base + 12 + j * 12;
            *v = Vec3::new(
                f32::from_le_bytes(data[off..off + 4].try_into().unwrap()),
                f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap()),
                f32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap()),
            );
        }
        mesh.add_vertex(vertices[0], Vec3::ZERO, crate::Vec2::ZERO);
        mesh.add_vertex(vertices[1], Vec3::ZERO, crate::Vec2::ZERO);
        mesh.add_vertex(vertices[2], Vec3::ZERO, crate::Vec2::ZERO);
        mesh.push_triangle(i as u32 * 3, i as u32 * 3 + 1, i as u32 * 3 + 2);
    }
    mesh.compute_smooth_normals();
    let mut out = ImportedScene::default();
    out.meshes.push(ImportMesh {
        name: "STL mesh".into(),
        mesh,
        color: Vec4::new(0.78, 0.78, 0.78, 1.0),
    });
    out.notes
        .push(format!("Imported {count} binary STL triangles"));
    Ok(out)
}

fn import_ascii(data: &[u8]) -> Result<ImportedScene> {
    let text = String::from_utf8_lossy(data);
    let mut mesh = Mesh::new();
    let mut pending: Vec<Vec3> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("vertex ") {
            let mut it = rest
                .split_whitespace()
                .filter_map(|v| v.parse::<f32>().ok());
            let p = Vec3::new(
                it.next().unwrap_or(0.0),
                it.next().unwrap_or(0.0),
                it.next().unwrap_or(0.0),
            );
            pending.push(p);
            if pending.len() == 3 {
                let base = mesh.positions.len() as u32;
                for v in pending.drain(..) {
                    mesh.add_vertex(v, Vec3::ZERO, crate::Vec2::ZERO);
                }
                mesh.push_triangle(base, base + 1, base + 2);
            }
        }
    }
    if mesh.is_empty() {
        return Err(PlumeError::Invalid("STL has no triangles".into()));
    }
    mesh.compute_smooth_normals();
    let mut out = ImportedScene::default();
    out.meshes.push(ImportMesh {
        name: "STL mesh".into(),
        mesh,
        color: Vec4::new(0.78, 0.78, 0.78, 1.0),
    });
    out.notes.push("Imported ASCII STL".into());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    fn document() -> Document {
        let mut doc = Document::new("STL test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            Vec3::ZERO,
            Vec3::new(30.0, 4.0, 0.0),
            Brush::default(),
        ));
        doc
    }

    #[test]
    fn binary_stl_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.stl");
        let report = export_stl(&document(), &path, &ExportOptions::default(), true).unwrap();
        assert!(report.triangles > 0);
        let data = std::fs::read(&path).unwrap();
        let count = u32::from_le_bytes(data[80..84].try_into().unwrap()) as usize;
        assert_eq!(84 + count * 50, data.len());
        let imported = import_stl(&path).unwrap();
        assert_eq!(imported.meshes[0].mesh.triangle_count(), count);
    }

    #[test]
    fn ascii_stl_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene_ascii.stl");
        export_stl(&document(), &path, &ExportOptions::default(), false).unwrap();
        let imported = import_stl(&path).unwrap();
        assert!(imported.meshes[0].mesh.triangle_count() > 0);
    }
}
