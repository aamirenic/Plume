//! Best-effort Feather document interchange.
//!
//! Feather's native document format is not publicly documented. PLUME can
//! recover geometry from JSON-based `.feather` archives when they contain
//! recognisable point arrays, and writes an archival package containing both a
//! full PLUME document and a simple, documented curve JSON. Unknown data is
//! preserved as notes rather than guessed at silently.

use crate::brush::Brush;
use crate::curve::{ControlPoint, Curve, CurveKind};
use crate::guide::Guide;
use crate::mesh::Mesh;
use crate::scene::Document;
use crate::{PlumeError, Result, Vec3, Vec4};
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

#[derive(Clone, Debug)]
pub struct FeatherImport {
    pub document: Document,
    pub notes: Vec<String>,
    pub skipped_entries: Vec<String>,
}

pub fn import_feather(path: &Path) -> Result<FeatherImport> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut curves: Vec<Curve> = Vec::new();
    let mut notes = Vec::new();
    let mut skipped = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut imported_meshes: Vec<(String, Mesh)> = Vec::new();
    let mut next_id = 1u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().map_err(PlumeError::Zip)?.to_string();
        if entry.size() > 128 * 1024 * 1024 {
            skipped.push(name);
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".json") || lower.ends_with(".feather") {
            if let Ok(value) = serde_json::from_slice::<Value>(&data) {
                let mut found = Vec::new();
                collect_curves(&value, &mut found, &mut seen, 0);
                for c in found {
                    let mut curve = c;
                    curve.id = next_id;
                    next_id += 1;
                    curves.push(curve);
                }
            } else {
                skipped.push(name);
            }
        } else if lower.ends_with(".obj") {
            if let Ok(obj) = obj::ObjData::load_buf(std::io::Cursor::new(&data)) {
                let mut mesh = crate::mesh::Mesh::new();
                for object in &obj.objects {
                    for group in &object.groups {
                        for poly in &group.polys {
                            if poly.0.len() < 3 {
                                continue;
                            }
                            let base = mesh.positions.len() as u32;
                            for vertex in &poly.0 {
                                let p = obj.position[vertex.0];
                                mesh.add_vertex(
                                    Vec3::new(p[0], p[1], p[2]),
                                    Vec3::ZERO,
                                    crate::Vec2::ZERO,
                                );
                            }
                            for j in 1..poly.0.len() - 1 {
                                mesh.push_triangle(base, base + j as u32, base + j as u32 + 1);
                            }
                        }
                    }
                }
                if !mesh.is_empty() {
                    mesh.compute_smooth_normals();
                    notes.push(format!("Recovered mesh guide from {name}"));
                    imported_meshes.push((name.clone(), mesh));
                }
            }
        } else if lower.ends_with(".png") || lower.ends_with(".jpg") {
            // Thumbnails are intentionally ignored; they are not geometry.
        } else {
            skipped.push(name);
        }
    }
    if curves.is_empty() && imported_meshes.is_empty() {
        return Err(PlumeError::Invalid(
            "no recoverable curves found in .feather archive (the format is undocumented); try exporting from Feather as OBJ or glTF"
                .into(),
        ));
    }
    let mut doc = Document::new(
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Imported Feather scene"),
    );
    for curve in curves {
        doc.scene.add_curve(curve);
    }
    for (source, mesh) in imported_meshes {
        let label = Path::new(&source)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("mesh");
        let mut guide = Guide::imported(
            doc.scene.new_guide_id(),
            format!("Imported {label}"),
            mesh,
            Some(source),
        );
        guide.solid = true;
        guide.opacity = 0.85;
        guide.color = Vec4::new(0.80, 0.82, 0.78, 1.0);
        doc.scene.add_guide(guide);
    }
    // Imported geometry must not silently become the drawing target.
    doc.scene.set_active_guide(None);
    notes.push(
        "Feather import is best-effort; brush dynamics, native guide metadata and hidden layers may not transfer."
            .into(),
    );
    Ok(FeatherImport {
        document: doc,
        notes,
        skipped_entries: skipped,
    })
}

fn collect_curves(value: &Value, out: &mut Vec<Curve>, seen: &mut BTreeSet<String>, depth: u32) {
    if depth > 24 {
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                collect_curves(item, out, seen, depth + 1);
            }
        }
        Value::Object(map) => {
            let candidate_keys = ["curves", "strokes", "paths", "lines", "splines"];
            for key in candidate_keys {
                if let Some(Value::Array(items)) = map.get(key) {
                    for item in items {
                        if let Some(curve) = curve_from_value(item) {
                            if let Some(key) = curve_key(&curve) {
                                if seen.insert(key) {
                                    out.push(curve);
                                }
                            }
                        }
                    }
                }
            }
            // An object that itself looks like a curve.
            if map.contains_key("points") || map.contains_key("controlPoints") {
                if let Some(curve) = curve_from_value(value) {
                    if let Some(key) = curve_key(&curve) {
                        if seen.insert(key) {
                            out.push(curve);
                        }
                    }
                }
            }
            for (key, child) in map {
                if candidate_keys.contains(&key.as_str())
                    || key == "points"
                    || key == "controlPoints"
                {
                    continue;
                }
                collect_curves(child, out, seen, depth + 1);
            }
        }
        _ => {}
    }
}

fn curve_key(curve: &Curve) -> Option<String> {
    if curve.points.is_empty() {
        return None;
    }
    let mut s = String::new();
    for p in curve.points.iter().take(24) {
        s.push_str(&format!(
            "{:.3},{:.3},{:.3};",
            p.position.x, p.position.y, p.position.z
        ));
    }
    Some(s)
}

fn curve_from_value(value: &Value) -> Option<Curve> {
    let obj = value.as_object()?;
    let points_value = obj
        .get("points")
        .or_else(|| obj.get("controlPoints"))
        .or_else(|| obj.get("control_points"))?;
    let array = points_value.as_array()?;
    let mut points = Vec::new();
    for item in array {
        if let Some((pos, pressure)) = point_from_value(item) {
            points.push(ControlPoint::new(pos, pressure));
        }
    }
    if points.len() < 2 {
        return None;
    }
    let name = obj
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Feather stroke");
    let brush = obj
        .get("brush")
        .and_then(|b| {
            if let Some(s) = b.as_str() {
                Brush::from_json_str(s).ok()
            } else {
                serde_json::from_value::<Brush>(b.clone()).ok()
            }
        })
        .unwrap_or_default();
    let color = obj
        .get("color")
        .and_then(color_from_value)
        .or_else(|| {
            obj.get("stroke")
                .and_then(|s| s.get("color"))
                .and_then(color_from_value)
        })
        .unwrap_or(brush.color);
    let mut curve = Curve::new(0, points, brush);
    curve.name = name.to_string();
    curve.color = color;
    if obj.get("closed").and_then(Value::as_bool).unwrap_or(false)
        || obj
            .get("isClosed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        curve.closed = true;
    }
    let kind = obj.get("kind").and_then(Value::as_str).unwrap_or("");
    curve.kind = match kind.to_ascii_lowercase().as_str() {
        "line" => CurveKind::Line,
        "circle" => {
            curve.closed = true;
            CurveKind::Circle
        }
        "ellipse" => {
            curve.closed = true;
            CurveKind::Ellipse
        }
        "polygon" => {
            curve.closed = true;
            CurveKind::Polygon
        }
        "bezier" => CurveKind::Bezier,
        _ => CurveKind::Freehand,
    };
    curve.refresh_frames();
    Some(curve)
}

fn point_from_value(value: &Value) -> Option<(Vec3, f32)> {
    let (position, pressure) = if let Some(obj) = value.as_object() {
        let p = if let Some(pos) = obj.get("position") {
            vec3_from_value(pos)?
        } else if let (Some(x), Some(y)) = (obj.get("x"), obj.get("y")) {
            Vec3::new(
                x.as_f64().unwrap_or(0.0) as f32,
                y.as_f64().unwrap_or(0.0) as f32,
                obj.get("z").and_then(Value::as_f64).unwrap_or(0.0) as f32,
            )
        } else if obj.contains_key("0") {
            vec3_from_value(value)?
        } else {
            return None;
        };
        let pressure = obj
            .get("pressure")
            .or_else(|| obj.get("p"))
            .and_then(Value::as_f64)
            .unwrap_or(0.8) as f32;
        (p, pressure)
    } else {
        (vec3_from_value(value)?, 0.8)
    };
    Some((position, pressure.clamp(0.0, 1.0)))
}

fn vec3_from_value(value: &Value) -> Option<Vec3> {
    let arr = value.as_array()?;
    if arr.len() < 2 {
        return None;
    }
    Some(Vec3::new(
        arr.first()?.as_f64()? as f32,
        arr.get(1)?.as_f64()? as f32,
        arr.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32,
    ))
}

fn color_from_value(value: &Value) -> Option<Vec4> {
    if let Some(arr) = value.as_array() {
        let r = arr.first()?.as_f64()? as f32;
        let g = arr.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let b = arr.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let a = arr.get(3).and_then(Value::as_f64).unwrap_or(1.0) as f32;
        // Feather may store 0..255 colours.
        if r > 1.5 || g > 1.5 || b > 1.5 {
            return Some(Vec4::new(r / 255.0, g / 255.0, b / 255.0, a));
        }
        return Some(Vec4::new(r, g, b, a));
    }
    if let Some(obj) = value.as_object() {
        let r = obj.get("r").and_then(Value::as_f64)? as f32;
        let g = obj.get("g").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let b = obj.get("b").and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let a = obj.get("a").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        if r > 1.5 || g > 1.5 || b > 1.5 {
            return Some(Vec4::new(r / 255.0, g / 255.0, b / 255.0, a));
        }
        return Some(Vec4::new(r, g, b, a));
    }
    None
}

/// Write a best-effort `.feather` interchange archive. It contains a complete
/// PLUME document, a documented curve JSON, and a README explaining that the
/// archive is not a byte-for-byte Feather document.
pub fn export_feather(
    document: &Document,
    path: &Path,
    thumbnail: Option<&image::RgbaImage>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    writer.start_file("document.json", options)?;
    writer.write_all(&serde_json::to_vec_pretty(document)?)?;
    writer.start_file("curves.json", options)?;
    let curves: Vec<Value> = document
        .scene
        .curves
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "closed": c.closed,
                "kind": format!("{:?}", c.kind).to_lowercase(),
                "color": [c.color.x, c.color.y, c.color.z, c.color.w],
                "points": c.points.iter().map(|p| serde_json::json!({
                    "position": [p.position.x, p.position.y, p.position.z],
                    "pressure": p.pressure,
                })).collect::<Vec<_>>()
            })
        })
        .collect();
    writer.write_all(&serde_json::to_vec_pretty(
        &serde_json::json!({ "curves": curves }),
    )?)?;
    if let Some(thumb) = thumbnail {
        let mut png = Vec::new();
        thumb
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(PlumeError::Image)?;
        writer.start_file("thumbnail.png", options)?;
        writer.write_all(&png)?;
    }
    writer.start_file("README.txt", options)?;
    writer.write_all(
        b"PLUME interchange archive.\n\
          This is not a native Feather document. It contains a full PLUME\n\
          document plus a simple curve JSON. Use document.json with PLUME or\n\
          curves.json with any tool that accepts JSON point arrays.\n",
    )?;
    let bytes = writer.finish().map_err(PlumeError::Zip)?.into_inner();
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    #[test]
    fn recovers_curves_from_json_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.feather");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .start_file("scene.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(br#"{"curves":[{"name":"Loop","points":[{"position":[0,0,0],"pressure":0.8},{"position":[10,4,0],"pressure":0.9},{"position":[20,0,0],"pressure":0.7}],"color":[255,90,40,1]}]}"#)
            .unwrap();
        writer.finish().unwrap();
        let import = import_feather(&path).unwrap();
        assert_eq!(import.document.scene.curves.len(), 1);
        let c = &import.document.scene.curves[0];
        assert_eq!(c.points.len(), 3);
        assert!((c.color.x - 1.0).abs() < 1e-5);
    }

    #[test]
    fn export_then_import_roundtrips_points() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.feather");
        let mut doc = Document::new("Feather test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            Vec3::ZERO,
            Vec3::new(12.0, 4.0, 0.0),
            Brush::default(),
        ));
        export_feather(&doc, &path, None).unwrap();
        let import = import_feather(&path).unwrap();
        assert_eq!(import.document.scene.curves.len(), 1);
        let pts = &import.document.scene.curves[0].points;
        assert_eq!(pts.len(), 2);
    }

    #[test]
    fn recovers_obj_geometry_as_an_imported_guide() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mesh.feather");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .start_file("props/stool.obj", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"v 0 0 0\nv 20 0 0\nv 0 20 0\nvn 0 0 1\nf 1//1 2//1 3//1\n")
            .unwrap();
        writer.finish().unwrap();

        let import = import_feather(&path).unwrap();
        assert!(import.document.scene.curves.is_empty());
        assert_eq!(import.document.scene.guides.len(), 1);
        let guide = &import.document.scene.guides[0];
        assert_eq!(guide.name, "Imported stool");
        match &guide.kind {
            crate::guide::GuideKind::Imported { source_path, mesh } => {
                assert_eq!(source_path.as_deref(), Some("props/stool.obj"));
                assert_eq!(mesh.triangle_count(), 1);
            }
            other => panic!("expected imported guide, got {other:?}"),
        }
        assert!(import
            .notes
            .iter()
            .any(|note| note.contains("Recovered mesh")));
        assert!(import.document.scene.active_guide.is_none());
    }

    #[test]
    fn empty_archive_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.feather");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        writer
            .start_file("meta.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(br#"{"name":"empty"}"#).unwrap();
        writer.finish().unwrap();
        assert!(import_feather(&path).is_err());
    }
}
