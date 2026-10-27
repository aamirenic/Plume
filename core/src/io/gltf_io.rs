//! glTF 2.0 writer (GLB and embedded .gltf) and reader.

use super::{
    collect_export_meshes, report_for, ExportOptions, ExportReport, ImportMesh, ImportedScene,
};
use crate::mesh::Mesh;
use crate::scene::Document;
use crate::{PlumeError, Result, Vec4};
use base64::Engine as _;
use serde_json::{json, Value};
use std::path::Path;

struct BufferView {
    offset: usize,
    length: usize,
    target: Option<u32>,
}

struct Builder {
    bin: Vec<u8>,
    views: Vec<BufferView>,
}

impl Builder {
    fn new() -> Self {
        Self {
            bin: Vec::new(),
            views: Vec::new(),
        }
    }

    fn align(&mut self, align: usize) {
        while !self.bin.len().is_multiple_of(align) {
            self.bin.push(0);
        }
    }

    fn push(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align(4);
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        self.views.push(BufferView {
            offset,
            length: bytes.len(),
            target,
        });
        self.views.len() - 1
    }
}

fn float_bytes(values: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 4);
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

pub fn export_glb(
    document: &Document,
    path: &Path,
    options: &ExportOptions,
) -> Result<ExportReport> {
    let gltf = build_gltf(document, options, false)?;
    let (json_chunk, bin_chunk) = match gltf {
        GltfOutput::Glb { json, bin } => (json, bin),
        GltfOutput::GltfEmbedded(_) => unreachable!(),
    };
    let mut json_bytes = serde_json::to_vec(&json_chunk)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin_chunk;
    while bin_bytes.len() % 4 != 0 {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin_bytes);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, out)?;
    let meshes = collect_export_meshes(document, options);
    Ok(report_for(path, &meshes))
}

pub fn export_gltf(
    document: &Document,
    path: &Path,
    options: &ExportOptions,
) -> Result<ExportReport> {
    match build_gltf(document, options, true)? {
        GltfOutput::GltfEmbedded(json) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, serde_json::to_vec_pretty(&json)?)?;
        }
        GltfOutput::Glb { .. } => unreachable!(),
    }
    let meshes = collect_export_meshes(document, options);
    Ok(report_for(path, &meshes))
}

enum GltfOutput {
    Glb { json: Value, bin: Vec<u8> },
    GltfEmbedded(Value),
}

fn build_gltf(document: &Document, options: &ExportOptions, embedded: bool) -> Result<GltfOutput> {
    let meshes = collect_export_meshes(document, options);
    if meshes.is_empty() {
        return Err(PlumeError::Invalid("nothing to export".into()));
    }
    let mut b = Builder::new();
    let mut accessors: Vec<Value> = Vec::new();
    let mut buffer_views: Vec<Value> = Vec::new();
    let mut materials: Vec<Value> = Vec::new();
    let mut gltf_meshes: Vec<Value> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();

    for (index, item) in meshes.iter().enumerate() {
        let m = &item.mesh;
        if m.is_empty() {
            continue;
        }
        let position_view = b.push(
            &float_bytes(
                &m.positions
                    .iter()
                    .flat_map(|p| [p.x, p.y, p.z])
                    .collect::<Vec<_>>(),
            ),
            Some(34962),
        );
        let position = add_accessor(
            &mut accessors,
            &mut buffer_views,
            &b,
            position_view,
            5126,
            "VEC3",
            m.positions.len(),
            Some(position_min_max(&m.positions)),
        );
        let mut attributes = serde_json::Map::new();
        attributes.insert("POSITION".into(), json!(position));
        if m.normals.len() == m.positions.len() {
            let view = b.push(
                &float_bytes(
                    &m.normals
                        .iter()
                        .flat_map(|n| [n.x, n.y, n.z])
                        .collect::<Vec<_>>(),
                ),
                Some(34962),
            );
            let normal = add_accessor(
                &mut accessors,
                &mut buffer_views,
                &b,
                view,
                5126,
                "VEC3",
                m.normals.len(),
                None,
            );
            attributes.insert("NORMAL".into(), json!(normal));
        }
        if m.uvs.len() == m.positions.len() {
            let view = b.push(
                &float_bytes(&m.uvs.iter().flat_map(|u| [u.x, u.y]).collect::<Vec<_>>()),
                Some(34962),
            );
            let uv = add_accessor(
                &mut accessors,
                &mut buffer_views,
                &b,
                view,
                5126,
                "VEC2",
                m.uvs.len(),
                None,
            );
            attributes.insert("TEXCOORD_0".into(), json!(uv));
        }
        let index_view = b.push(&u32_bytes(&m.indices), Some(34963));
        let indices = add_accessor(
            &mut accessors,
            &mut buffer_views,
            &b,
            index_view,
            5125,
            "SCALAR",
            m.indices.len(),
            None,
        );
        let color = item.color;
        let mut material = json!({
            "name": format!("PLUME material {index}"),
            "pbrMetallicRoughness": {
                "baseColorFactor": [color.x.clamp(0.0, 1.0), color.y.clamp(0.0, 1.0), color.z.clamp(0.0, 1.0), color.w.clamp(0.0, 1.0)],
                "metallicFactor": 0.05,
                "roughnessFactor": 0.62
            },
            "doubleSided": options.double_sided
        });
        if color.w < 0.999 {
            material["alphaMode"] = json!("BLEND");
        }
        if item.emission > 0.0 {
            let e = item.emission.max(0.0);
            material["emissiveFactor"] = json!([
                (color.x * e).min(1.0),
                (color.y * e).min(1.0),
                (color.z * e).min(1.0)
            ]);
            if e > 1.0 {
                material["extensions"] = json!({
                    "KHR_materials_emissive_strength": { "emissiveStrength": e }
                });
            }
        }
        materials.push(material);
        gltf_meshes.push(json!({
            "name": item.name,
            "primitives": [{
                "attributes": attributes,
                "indices": indices,
                "material": materials.len() - 1,
                "mode": 4
            }]
        }));
        nodes.push(json!({
            "name": item.name,
            "mesh": gltf_meshes.len() - 1
        }));
    }
    if accessors.is_empty() {
        return Err(PlumeError::Invalid("no exportable geometry".into()));
    }
    let node_indices: Vec<usize> = (0..nodes.len()).collect();
    let mut root = json!({
        "asset": {
            "version": "2.0",
            "generator": "PLUME",
            "copyright": "PLUME contributors"
        },
        "scene": 0,
        "scenes": [{ "name": document.name, "nodes": node_indices }],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "extensionsUsed": ["KHR_materials_emissive_strength"]
    });
    if embedded {
        root["buffers"] = json!([{
            "byteLength": b.bin.len(),
            "uri": format!("data:application/octet-stream;base64,{}", base64::engine::general_purpose::STANDARD.encode(&b.bin))
        }]);
        Ok(GltfOutput::GltfEmbedded(root))
    } else {
        root["buffers"] = json!([{ "byteLength": b.bin.len() }]);
        Ok(GltfOutput::Glb {
            json: root,
            bin: b.bin,
        })
    }
}

// The glTF accessor descriptor is a flat set of independent fields; grouping
// them into a throwaway struct would add indirection without adding meaning.
#[allow(clippy::too_many_arguments)]
fn add_accessor(
    accessors: &mut Vec<Value>,
    buffer_views: &mut Vec<Value>,
    builder: &Builder,
    view: usize,
    component_type: u32,
    ty: &str,
    count: usize,
    bounds: Option<([f32; 3], [f32; 3])>,
) -> usize {
    let bv = &builder.views[view];
    let mut view_json = json!({
        "buffer": 0,
        "byteOffset": bv.offset,
        "byteLength": bv.length
    });
    if let Some(target) = bv.target {
        view_json["target"] = json!(target);
    }
    buffer_views.push(view_json);
    let mut accessor = json!({
        "bufferView": buffer_views.len() - 1,
        "componentType": component_type,
        "count": count,
        "type": ty
    });
    if let Some((min, max)) = bounds {
        accessor["min"] = json!([min[0], min[1], min[2]]);
        accessor["max"] = json!([max[0], max[1], max[2]]);
    }
    accessors.push(accessor);
    accessors.len() - 1
}

fn position_min_max(positions: &[crate::Vec3]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        min[0] = min[0].min(p.x);
        min[1] = min[1].min(p.y);
        min[2] = min[2].min(p.z);
        max[0] = max[0].max(p.x);
        max[1] = max[1].max(p.y);
        max[2] = max[2].max(p.z);
    }
    if positions.is_empty() {
        ([0.0; 3], [0.0; 3])
    } else {
        (min, max)
    }
}

pub fn import_gltf(path: &Path) -> Result<ImportedScene> {
    let (document, buffers, _images) = gltf::import(path)?;
    let mut out = ImportedScene::default();
    for (mesh_index, gltf_mesh) in document.meshes().enumerate() {
        for (prim_index, primitive) in gltf_mesh.primitives().enumerate() {
            let reader =
                primitive.reader(|buffer| buffers.get(buffer.index()).map(|b| b.0.as_slice()));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<[f32; 3]> = positions.collect();
            if positions.is_empty() {
                continue;
            }
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uvs: Option<Vec<[f32; 2]>> =
                reader.read_tex_coords(0).map(|u| u.into_f32().collect());
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let mut mesh = Mesh::new();
            for (i, p) in positions.iter().enumerate() {
                let n = normals
                    .as_ref()
                    .and_then(|n| n.get(i))
                    .map(|n| crate::Vec3::new(n[0], n[1], n[2]).normalize_or_zero())
                    .unwrap_or(crate::Vec3::ZERO);
                let uv = uvs
                    .as_ref()
                    .and_then(|u| u.get(i))
                    .map(|u| crate::Vec2::new(u[0], u[1]))
                    .unwrap_or(crate::Vec2::ZERO);
                mesh.add_vertex(crate::Vec3::new(p[0], p[1], p[2]), n, uv);
            }
            mesh.indices = indices;
            if mesh.normals.iter().all(|n| n.length_squared() < 1.0e-8) {
                mesh.compute_smooth_normals();
            }
            let material = primitive.material();
            let c = material.pbr_metallic_roughness().base_color_factor();
            let color = Vec4::new(c[0], c[1], c[2], c[3]);
            let name = gltf_mesh
                .name()
                .map(|n| n.to_string())
                .unwrap_or_else(|| format!("glTF mesh {mesh_index}.{prim_index}"));
            out.meshes.push(ImportMesh { name, mesh, color });
        }
    }
    if out.meshes.is_empty() {
        return Err(PlumeError::Invalid(
            "glTF contains no triangle meshes".into(),
        ));
    }
    out.notes
        .push(format!("Imported {} glTF primitive(s)", out.meshes.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    fn document() -> Document {
        let mut doc = Document::new("glTF test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            crate::Vec3::ZERO,
            crate::Vec3::new(20.0, 5.0, 0.0),
            Brush::default(),
        ));
        doc
    }

    #[test]
    fn glb_header_is_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.glb");
        let opts = ExportOptions {
            unit_scale: 1.0,
            ..ExportOptions::default()
        };
        export_glb(&document(), &path, &opts).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 2);
        assert_eq!(
            data.len() as u32,
            u32::from_le_bytes(data[8..12].try_into().unwrap())
        );
        let imported = import_gltf(&path).unwrap();
        assert!(!imported.meshes.is_empty());
    }

    #[test]
    fn gltf_is_self_contained() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.gltf");
        let opts = ExportOptions {
            unit_scale: 1.0,
            ..ExportOptions::default()
        };
        export_gltf(&document(), &path, &opts).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("data:application/octet-stream;base64"));
        let imported = import_gltf(&path).unwrap();
        assert!(!imported.meshes.is_empty());
    }

    #[test]
    fn empty_scene_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        assert!(export_glb(
            &Document::new("empty"),
            &dir.path().join("e.glb"),
            &ExportOptions::default()
        )
        .is_err());
    }
}
