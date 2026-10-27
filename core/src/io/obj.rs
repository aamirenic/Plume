//! Wavefront OBJ/MTL writer and reader.

use super::{
    collect_export_meshes, report_for, ExportOptions, ExportReport, ImportMesh, ImportedScene,
};
use crate::mesh::Mesh;
use crate::scene::Document;
use crate::{PlumeError, Result, Vec4};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub fn export_obj(
    document: &Document,
    path: &Path,
    options: &ExportOptions,
) -> Result<ExportReport> {
    let meshes = collect_export_meshes(document, options);
    if meshes.is_empty() {
        return Err(PlumeError::Invalid("nothing to export".into()));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("plume");
    let mtl_path = path.with_extension("mtl");
    let mtl_name = mtl_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("plume.mtl");
    let mut obj = String::with_capacity(64 * 1024);
    let _ = writeln!(obj, "# PLUME scene export");
    let _ = writeln!(obj, "mtllib {mtl_name}");
    let mut mtl = String::new();
    let _ = writeln!(mtl, "# PLUME materials");
    let mut vertex_base = 1usize;
    let mut uv_base = 1usize;
    let mut normal_base = 1usize;
    for (index, item) in meshes.iter().enumerate() {
        let m = &item.mesh;
        if m.is_empty() {
            continue;
        }
        let material = format!("plume_{index}");
        let _ = writeln!(obj, "o {}", sanitize_obj(&item.name));
        let _ = writeln!(obj, "usemtl {material}");
        for p in &m.positions {
            let _ = writeln!(obj, "v {:.6} {:.6} {:.6}", p.x, p.y, p.z);
        }
        for uv in &m.uvs {
            let _ = writeln!(obj, "vt {:.6} {:.6}", uv.x, uv.y);
        }
        for n in &m.normals {
            let _ = writeln!(obj, "vn {:.6} {:.6} {:.6}", n.x, n.y, n.z);
        }
        let has_uv = m.uvs.len() == m.positions.len();
        let has_normal = m.normals.len() == m.positions.len();
        for tri in m.indices.as_chunks::<3>().0 {
            let a = tri[0] as usize + vertex_base;
            let b = tri[1] as usize + vertex_base;
            let c = tri[2] as usize + vertex_base;
            match (has_uv, has_normal) {
                (true, true) => {
                    let _ = writeln!(
                        obj,
                        "f {a}/{}/{n0} {b}/{}/{n1} {c}/{}/{n2}",
                        a - vertex_base + uv_base,
                        b - vertex_base + uv_base,
                        c - vertex_base + uv_base,
                        n0 = a - vertex_base + normal_base,
                        n1 = b - vertex_base + normal_base,
                        n2 = c - vertex_base + normal_base
                    );
                }
                (false, true) => {
                    let _ = writeln!(
                        obj,
                        "f {a}//{} {b}//{} {c}//{}",
                        a - vertex_base + normal_base,
                        b - vertex_base + normal_base,
                        c - vertex_base + normal_base
                    );
                }
                (true, false) => {
                    let _ = writeln!(
                        obj,
                        "f {a}/{} {b}/{} {c}/{}",
                        a - vertex_base + uv_base,
                        b - vertex_base + uv_base,
                        c - vertex_base + uv_base
                    );
                }
                (false, false) => {
                    let _ = writeln!(obj, "f {a} {b} {c}");
                }
            }
        }
        let c = item.color;
        let _ = writeln!(mtl, "newmtl {material}");
        let _ = writeln!(mtl, "Kd {:.5} {:.5} {:.5}", c.x, c.y, c.z);
        let _ = writeln!(mtl, "d {:.5}", c.w.clamp(0.0, 1.0));
        let _ = writeln!(mtl, "Ks 0.2 0.2 0.2");
        let _ = writeln!(mtl, "Ns 18");
        if item.emission > 0.0 {
            let _ = writeln!(
                mtl,
                "Ke {:.5} {:.5} {:.5}",
                c.x * item.emission,
                c.y * item.emission,
                c.z * item.emission
            );
        }
        vertex_base += m.positions.len();
        uv_base += m.uvs.len();
        normal_base += m.normals.len();
    }
    std::fs::write(path, obj)?;
    std::fs::write(&mtl_path, mtl)?;
    let _ = stem;
    Ok(report_for(path, &meshes))
}

fn sanitize_obj(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .filter(|c| c.is_ascii_graphic())
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

pub fn import_obj(path: &Path) -> Result<ImportedScene> {
    let data = std::fs::read_to_string(path)?;
    let obj = obj::ObjData::load_buf(std::io::Cursor::new(data.as_bytes()))
        .map_err(|e| PlumeError::Obj(e.to_string()))?;
    let colors = parse_mtl_colors(path);
    let mut out = ImportedScene::default();
    for object in &obj.objects {
        let name = if object.name.is_empty() {
            "Imported object".to_string()
        } else {
            object.name.clone()
        };
        let mut mesh = Mesh::new();
        let mut object_color = Vec4::ONE;
        for group in &object.groups {
            let group_color = match group.material.as_ref() {
                Some(obj::ObjMaterial::Ref(name)) => colors.get(name).copied().unwrap_or(Vec4::ONE),
                Some(obj::ObjMaterial::Mtl(mat)) => {
                    let kd = mat.kd.unwrap_or([1.0, 1.0, 1.0]);
                    let alpha = mat.d.unwrap_or(1.0);
                    Vec4::new(kd[0], kd[1], kd[2], alpha)
                }
                None => Vec4::ONE,
            };
            if object_color == Vec4::ONE {
                object_color = group_color;
            }
            for polygon in &group.polys {
                let vertices = &polygon.0;
                if vertices.len() < 3 {
                    continue;
                }
                let base = mesh.positions.len() as u32;
                for vertex in vertices {
                    let pos = obj.position[vertex.0];
                    let position = crate::Vec3::new(pos[0], pos[1], pos[2]);
                    let normal = vertex
                        .2
                        .and_then(|i| obj.normal.get(i))
                        .map(|n| crate::Vec3::new(n[0], n[1], n[2]).normalize_or_zero())
                        .unwrap_or(crate::Vec3::ZERO);
                    let uv = vertex
                        .1
                        .and_then(|i| obj.texture.get(i))
                        .map(|t| crate::Vec2::new(t[0], 1.0 - t[1]))
                        .unwrap_or(crate::Vec2::ZERO);
                    mesh.add_vertex(position, normal, uv);
                }
                for i in 1..vertices.len() - 1 {
                    mesh.push_triangle(base, base + i as u32, base + i as u32 + 1);
                }
            }
        }
        if mesh.positions.is_empty() {
            continue;
        }
        if mesh.normals.iter().all(|n| n.length_squared() < 1.0e-8) {
            mesh.compute_smooth_normals();
        }
        out.meshes.push(ImportMesh {
            name,
            mesh,
            color: object_color,
        });
    }
    if let Some(parent) = path.parent() {
        let _ = parent;
    }
    if out.meshes.is_empty() {
        return Err(PlumeError::Obj("OBJ has no faces".into()));
    }
    out.notes
        .push(format!("Imported {} OBJ object(s)", out.meshes.len()));
    Ok(out)
}

fn parse_mtl_colors(obj_path: &Path) -> std::collections::HashMap<String, Vec4> {
    let mut out = std::collections::HashMap::new();
    let mtl_path: PathBuf = match std::fs::read_to_string(obj_path) {
        Ok(text) => {
            let name = text.lines().find_map(|l| {
                l.trim()
                    .strip_prefix("mtllib ")
                    .map(|s| s.trim().to_string())
            });
            match name {
                Some(name) => obj_path.with_file_name(name),
                None => obj_path.with_extension("mtl"),
            }
        }
        Err(_) => return out,
    };
    let Ok(text) = std::fs::read_to_string(mtl_path) else {
        return out;
    };
    let mut current: Option<String> = None;
    let mut color = Vec4::ONE;
    let mut alpha = 1.0;
    let flush =
        |out: &mut std::collections::HashMap<String, Vec4>, name: &Option<String>, c: Vec4| {
            if let Some(name) = name {
                out.insert(name.clone(), c);
            }
        };
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("newmtl ") {
            if let Some(ref prev) = current {
                flush(
                    &mut out,
                    &Some(prev.clone()),
                    Vec4::new(color.x, color.y, color.z, alpha),
                );
            }
            current = Some(name.trim().to_string());
            color = Vec4::ONE;
            alpha = 1.0;
        } else if let Some(rest) = line.strip_prefix("Kd ") {
            let mut it = rest
                .split_whitespace()
                .filter_map(|v| v.parse::<f32>().ok());
            color.x = it.next().unwrap_or(1.0);
            color.y = it.next().unwrap_or(1.0);
            color.z = it.next().unwrap_or(1.0);
        } else if let Some(rest) = line.strip_prefix("d ") {
            alpha = rest.trim().parse().unwrap_or(1.0);
        }
    }
    flush(
        &mut out,
        &current,
        Vec4::new(color.x, color.y, color.z, alpha),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;
    use crate::scene::Document;

    fn document() -> Document {
        let mut doc = Document::new("OBJ test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            crate::Vec3::ZERO,
            crate::Vec3::new(50.0, 0.0, 0.0),
            Brush::default(),
        ));
        doc
    }

    #[test]
    fn obj_roundtrip_has_faces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.obj");
        let report = export_obj(&document(), &path, &ExportOptions::default()).unwrap();
        assert!(report.triangles > 0);
        assert!(path.exists());
        let imported = import_obj(&path).unwrap();
        assert!(!imported.meshes.is_empty());
        assert!(imported.meshes[0].mesh.triangle_count() > 0);
    }

    #[test]
    fn mtl_sidecar_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shaded.obj");
        export_obj(&document(), &path, &ExportOptions::default()).unwrap();
        let mtl = std::fs::read_to_string(path.with_extension("mtl")).unwrap();
        assert!(mtl.contains("newmtl"));
        assert!(mtl.contains("Kd"));
    }

    #[test]
    fn empty_document_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let doc = Document::new("empty");
        assert!(export_obj(
            &doc,
            &dir.path().join("empty.obj"),
            &ExportOptions::default()
        )
        .is_err());
    }
}
