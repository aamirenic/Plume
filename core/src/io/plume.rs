//! `.plume` document package (ZIP) and lightweight manifest.

use crate::scene::{Document, PLUME_FORMAT_VERSION, PLUME_SCHEMA};
use crate::{PlumeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use zip::write::SimpleFileOptions;

pub const PLUME_PACKAGE_SCHEMA: &str = "plume.package";
pub const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_ARCHIVE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PackageManifest {
    pub schema: String,
    pub version: u32,
    pub application: String,
    pub document: String,
    pub thumbnail: Option<String>,
    pub assets: Vec<String>,
    pub created: String,
}

impl Default for PackageManifest {
    fn default() -> Self {
        Self {
            schema: PLUME_PACKAGE_SCHEMA.into(),
            version: 1,
            application: "PLUME".into(),
            document: "document.json".into(),
            thumbnail: Some("thumbnail.png".into()),
            assets: Vec::new(),
            created: utc_timestamp(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlumePackage {
    pub document: Document,
    pub thumbnail: Option<image::RgbaImage>,
    pub assets: BTreeMap<String, Vec<u8>>,
    pub manifest: PackageManifest,
    pub notes: Vec<String>,
}

/// Replace a path's extension with the native `.plume` package extension.
pub fn package_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("plume")
}

pub fn save_plume(
    path: &Path,
    document: &Document,
    thumbnail: Option<&image::RgbaImage>,
    assets: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    let bytes = plume_bytes(document, thumbnail, assets)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write to a sibling first, flush it, then rename over the destination.
    // `rename` replaces an existing file atomically on both Unix and Windows,
    // so a crash can leave the old package intact but never a partial one.
    let tmp = path.with_extension(format!("plume.{}.tmp", std::process::id()));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    if let Err(error) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}

pub fn plume_bytes(
    document: &Document,
    thumbnail: Option<&image::RgbaImage>,
    assets: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>> {
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(6));
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut doc = document.clone();
    doc.thumbnail = None;
    doc.modified = utc_timestamp();
    let doc_json = serde_json::to_vec_pretty(&doc)?;
    writer.start_file("document.json", options)?;
    writer.write_all(&doc_json)?;
    if let Some(img) = thumbnail {
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(PlumeError::Image)?;
        writer.start_file("thumbnail.png", options)?;
        writer.write_all(&png)?;
    }
    let mut manifest = PackageManifest {
        created: doc.created.clone(),
        ..PackageManifest::default()
    };
    for (name, data) in assets {
        let clean = sanitize_entry(name)?;
        manifest.assets.push(clean.clone());
        writer.start_file(format!("assets/{clean}"), options)?;
        writer.write_all(data)?;
    }
    writer.start_file("manifest.json", options)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    let cursor = writer.finish().map_err(PlumeError::Zip)?;
    Ok(cursor.into_inner())
}

pub fn load_plume(path: &Path) -> Result<PlumePackage> {
    let file = std::fs::File::open(path)?;
    load_plume_reader(file)
}

pub fn load_plume_reader<R: Read + std::io::Seek>(reader: R) -> Result<PlumePackage> {
    let mut zip = zip::ZipArchive::new(reader)?;
    let mut document_data = None;
    let mut thumbnail = None;
    let mut manifest = PackageManifest::default();
    let mut found_manifest = false;
    let mut assets = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let name = file.name().map_err(PlumeError::Zip)?.to_string();
        if name.contains("..") || name.starts_with('/') || name.contains('\\') {
            return Err(PlumeError::Invalid(format!("unsafe archive path: {name}")));
        }
        total = total.saturating_add(file.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err(PlumeError::Invalid(
                "archive expands past safety limit".into(),
            ));
        }
        if file.size() > MAX_ENTRY_BYTES {
            return Err(PlumeError::Invalid(format!("entry {name} is too large")));
        }
        let mut data = Vec::with_capacity(file.size().min(16 * 1024 * 1024) as usize);
        file.read_to_end(&mut data)?;
        match name.as_str() {
            "document.json" => document_data = Some(data),
            "thumbnail.png" => {
                thumbnail = image::load_from_memory(&data).ok().map(|i| i.to_rgba8());
            }
            "manifest.json" => {
                manifest = serde_json::from_slice(&data)?;
                found_manifest = true;
            }
            _ => {
                if let Some(stripped) = name.strip_prefix("assets/") {
                    assets.insert(stripped.to_string(), data);
                }
            }
        }
    }
    let data = document_data.ok_or_else(|| PlumeError::Invalid("missing document.json".into()))?;
    let mut document: Document = serde_json::from_slice(&data)?;
    if document.schema != PLUME_SCHEMA {
        return Err(PlumeError::Invalid(format!(
            "not a PLUME document: schema {}",
            document.schema
        )));
    }
    if document.version > PLUME_FORMAT_VERSION {
        return Err(PlumeError::Invalid(format!(
            "document version {} is newer than this build supports",
            document.version
        )));
    }
    document.thumbnail = thumbnail.clone();
    let mut notes = Vec::new();
    if !found_manifest {
        notes.push("archive has no manifest; recovered document.json".into());
    }
    // Rebuild guide caches that are skipped by serialisation.
    for guide in &mut document.scene.guides {
        guide.rebuild();
    }
    if !assets.is_empty() {
        notes.push(format!("loaded {} custom asset(s)", assets.len()));
    }
    Ok(PlumePackage {
        document,
        thumbnail,
        assets,
        manifest,
        notes,
    })
}

fn sanitize_entry(name: &str) -> Result<String> {
    let clean = name.replace('\\', "/");
    if clean.is_empty() || clean.starts_with('/') || clean.contains("..") {
        return Err(PlumeError::Invalid(format!("invalid asset path: {name}")));
    }
    Ok(clean)
}

/// Backwards-compatible re-export used by the app, FFI and tests.
pub use crate::time::utc_timestamp;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::curve::Curve;

    fn document() -> Document {
        let mut doc = Document::new("Package test");
        let id = doc.scene.new_curve_id();
        doc.scene.add_curve(Curve::line(
            id,
            crate::Vec3::ZERO,
            crate::Vec3::new(20.0, 0.0, 0.0),
            Brush::default(),
        ));
        doc
    }

    #[test]
    fn package_roundtrip_preserves_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.plume");
        let mut assets = BTreeMap::new();
        assets.insert("tiles/paper.png".into(), vec![1, 2, 3, 4]);
        save_plume(&path, &document(), None, &assets).unwrap();
        let package = load_plume(&path).unwrap();
        assert_eq!(package.document.scene.curves.len(), 1);
        assert_eq!(package.assets.len(), 1);
        assert!(package.notes.iter().any(|n| n.contains("custom asset")));
    }

    #[test]
    fn timestamp_is_iso_like() {
        let t = utc_timestamp();
        assert_eq!(t.len(), 20);
        assert!(t.ends_with('Z'));
        assert_eq!(&t[4..5], "-");
    }

    #[test]
    fn package_path_replaces_the_extension() {
        assert_eq!(
            package_path(Path::new("sketch.feather")),
            std::path::PathBuf::from("sketch.plume")
        );
    }

    #[test]
    fn save_replaces_an_existing_package_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("replace.plume");
        let mut first = document();
        first.name = "First".into();
        save_plume(&path, &first, None, &BTreeMap::new()).unwrap();
        let mut second = document();
        second.name = "Second".into();
        save_plume(&path, &second, None, &BTreeMap::new()).unwrap();
        assert_eq!(load_plume(&path).unwrap().document.name, "Second");
        assert!(
            !std::fs::read_dir(dir.path()).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")),
            "temporary package should be gone after a successful save",
        );
    }

    #[test]
    fn thumbnail_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("thumb.plume");
        let thumb = image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 34, 56, 255]));
        save_plume(&path, &document(), Some(&thumb), &BTreeMap::new()).unwrap();
        let package = load_plume(&path).unwrap();
        assert!(package.thumbnail.is_some());
        assert_eq!(package.thumbnail.unwrap().dimensions(), (8, 8));
    }

    #[test]
    fn missing_document_is_rejected() {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("readme.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"nope").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(load_plume_reader(std::io::Cursor::new(bytes)).is_err());
    }
}
