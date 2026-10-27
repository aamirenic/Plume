//! USDZ writer built directly from the published ZIP + USDA specifications.
//!
//! USDZ is a ZIP archive whose payloads are stored (not compressed) and whose
//! local file data starts on a 64-byte boundary. The geometry is an ASCII USDA
//! mesh hierarchy with vertex normals and UVs, which every USD-compatible
//! DCC can open without any PLUME-specific plugin.

use super::{collect_export_meshes, ExportOptions, ExportReport};
use crate::scene::Document;
use crate::{PlumeError, Result};
use std::path::Path;

struct Entry {
    name: String,
    data: Vec<u8>,
}

pub fn export_usdz(
    document: &Document,
    path: &Path,
    options: &ExportOptions,
) -> Result<ExportReport> {
    let meshes = collect_export_meshes(document, options);
    if meshes.is_empty() {
        return Err(PlumeError::Invalid("nothing to export".into()));
    }
    let usda = build_usda(document, &meshes);
    let entries = vec![Entry {
        name: format!("{}.usda", super::sanitize_name(&document.name)),
        data: usda.into_bytes(),
    }];
    let bytes = build_zip(&entries)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, &bytes)?;
    Ok(ExportReport {
        path: path.to_path_buf(),
        objects: meshes.len(),
        vertices: meshes.iter().map(|m| m.mesh.vertex_count()).sum(),
        triangles: meshes.iter().map(|m| m.mesh.triangle_count()).sum(),
        bytes: bytes.len() as u64,
    })
}

fn build_usda(document: &Document, meshes: &[super::ExportMesh]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(256 * 1024);
    let _ = writeln!(out, "#usda 1.0");
    let _ = writeln!(out, "(");
    let _ = writeln!(out, "    defaultPrim = \"PLUME\"");
    let _ = writeln!(out, "    metersPerUnit = 1");
    let _ = writeln!(out, "    upAxis = \"Y\"");
    let _ = writeln!(out, "    doc = \"{}\"", escape_usd(&document.name));
    let _ = writeln!(out, ")");
    let _ = writeln!(out);
    let _ = writeln!(out, "def Xform \"PLUME\" {{");
    for (index, item) in meshes.iter().enumerate() {
        let mesh = &item.mesh;
        if mesh.is_empty() {
            continue;
        }
        let name = format!(
            "curve_{index}_{}",
            item.name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        );
        let _ = writeln!(out, "    def Mesh \"{name}\" {{");
        let _ = writeln!(
            out,
            "        int[] faceVertexCounts = [{}]",
            vec!["3"; mesh.triangle_count()].join(", ")
        );
        let _ = writeln!(
            out,
            "        int[] faceVertexIndices = [{}]",
            mesh.indices
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let _ = writeln!(out, "        point3f[] points = [");
        for p in &mesh.positions {
            let _ = writeln!(out, "            ({:.6}, {:.6}, {:.6}),", p.x, p.y, p.z);
        }
        let _ = writeln!(out, "        ]");
        if mesh.normals.len() == mesh.positions.len() {
            let _ = writeln!(out, "        normal3f[] normals = [");
            for n in &mesh.normals {
                let _ = writeln!(out, "            ({:.6}, {:.6}, {:.6}),", n.x, n.y, n.z);
            }
            let _ = writeln!(out, "        ] (interpolation = \"vertex\")");
        }
        if mesh.uvs.len() == mesh.positions.len() {
            let _ = writeln!(out, "        texCoord2f[] primvars:st = [");
            for uv in &mesh.uvs {
                let _ = writeln!(out, "            ({:.6}, {:.6}),", uv.x, uv.y);
            }
            let _ = writeln!(out, "        ] (interpolation = \"vertex\")");
        }
        let _ = writeln!(
            out,
            "        color3f[] primvars:displayColor = [({:.5}, {:.5}, {:.5})] (interpolation = \"constant\")",
            item.color.x, item.color.y, item.color.z
        );
        let _ = writeln!(out, "        uniform token subdivisionScheme = \"none\"");
        let _ = writeln!(
            out,
            "        uniform token[] xformOpOrder = [\"xformOp:transform\"]"
        );
        let _ = writeln!(
            out,
            "        float4 xformOp:transform = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)"
        );
        let _ = writeln!(out, "    }}");
    }
    let _ = writeln!(out, "}}");
    out
}

fn escape_usd(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn build_zip(entries: &[Entry]) -> Result<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<(String, u32, usize, usize, usize)> = Vec::new();
    for entry in entries {
        let name = entry.name.as_bytes();
        if name.len() > u16::MAX as usize {
            return Err(PlumeError::Invalid("USDZ entry name too long".into()));
        }
        let crc = crc32(&entry.data);
        let local_offset = out.len();
        // Always align the payload, adding 64 bytes when the required padding
        // is too small to hold an extra-field header.
        let required = (64 - ((local_offset + 30 + name.len()) % 64)) % 64;
        let extra_total = if required < 4 {
            required + 64
        } else {
            required
        };
        let extra_data_len = extra_total.saturating_sub(4);
        write_u32(&mut out, 0x0403_4b50);
        write_u16(&mut out, 10);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, crc);
        write_u32(&mut out, entry.data.len() as u32);
        write_u32(&mut out, entry.data.len() as u32);
        write_u16(&mut out, name.len() as u16);
        write_u16(&mut out, extra_total as u16);
        out.extend_from_slice(name);
        if extra_total >= 4 {
            write_u16(&mut out, 0x1986);
            write_u16(&mut out, extra_data_len as u16);
            out.resize(out.len() + extra_data_len, 0);
        }
        debug_assert_eq!(out.len() % 64, 0, "USDZ payload must be aligned");
        out.extend_from_slice(&entry.data);
        central.push((
            entry.name.clone(),
            crc,
            local_offset,
            entry.data.len(),
            extra_total,
        ));
    }
    let cd_start = out.len();
    for (name, crc, offset, size, extra_total) in &central {
        let name_bytes = name.as_bytes();
        write_u32(&mut out, 0x0201_4b50);
        write_u16(&mut out, 20);
        write_u16(&mut out, 20);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, *crc);
        write_u32(&mut out, *size as u32);
        write_u32(&mut out, *size as u32);
        write_u16(&mut out, name_bytes.len() as u16);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u16(&mut out, 0);
        write_u32(&mut out, 0);
        write_u32(&mut out, *offset as u32);
        out.extend_from_slice(name_bytes);
        let _ = extra_total;
    }
    let cd_size = out.len() - cd_start;
    write_u32(&mut out, 0x0605_4b50);
    write_u16(&mut out, 0);
    write_u16(&mut out, 0);
    write_u16(&mut out, central.len() as u16);
    write_u16(&mut out, central.len() as u16);
    write_u32(&mut out, cd_size as u32);
    write_u32(&mut out, cd_start as u32);
    write_u16(&mut out, 0);
    Ok(out)
}

fn write_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;
    use std::io::Read as _;

    fn document() -> Document {
        let mut doc = Document::new("USDZ test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            crate::Vec3::ZERO,
            crate::Vec3::new(20.0, 3.0, 0.0),
            Brush::default(),
        ));
        doc
    }

    #[test]
    fn usdz_is_a_valid_zip_with_64_byte_alignment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scene.usdz");
        let opts = ExportOptions {
            unit_scale: 1.0,
            ..ExportOptions::default()
        };
        export_usdz(&document(), &path, &opts).unwrap();
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[0..4], &0x0403_4b50u32.to_le_bytes());
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data.clone())).unwrap();
        assert_eq!(zip.len(), 1);
        let mut usda = String::new();
        zip.by_index(0).unwrap().read_to_string(&mut usda).unwrap();
        assert!(usda.contains("def Mesh"));
        assert!(usda.contains("faceVertexIndices"));
        // Local header: name length at 26, extra length at 28, name starts at 30.
        let name_len = u16::from_le_bytes(data[26..28].try_into().unwrap()) as usize;
        let extra_len = u16::from_le_bytes(data[28..30].try_into().unwrap()) as usize;
        let payload = 30 + name_len + extra_len;
        assert_eq!(payload % 64, 0);
    }

    #[test]
    fn usda_contains_normals() {
        let doc = document();
        let meshes = collect_export_meshes(&doc, &ExportOptions::default());
        let usda = build_usda(&doc, &meshes);
        assert!(usda.contains("normal3f[] normals"));
        assert!(usda.contains("point3f[] points"));
        assert!(usda.contains("metersPerUnit = 1"));
    }
}
