//! Regenerate the committed sample documents, previews and interchange files.
//!
//! Every file under `samples/` and `testdata/` is produced by PLUME's own
//! gallery, package writer and exporters. Run from the workspace root:
//!
//! ```text
//! cargo run -p plume-core --example generate_samples
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};

use plume_core::gallery;
use plume_core::guide::Guide;
use plume_core::io::{self, ExportOptions};
use plume_core::mesh::Mesh;
use plume_core::scene::{Document, MeshQuality};
use plume_core::{Vec2, Vec3};

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("core lives inside the workspace")
        .to_path_buf();
    let samples = root.join("samples");
    let documents = samples.join("documents");
    let previews = samples.join("previews");
    let interchange = samples.join("interchange");
    let testdata = root.join("testdata");
    for dir in [&documents, &previews, &interchange, &testdata] {
        std::fs::create_dir_all(dir)?;
    }

    // A compact, deterministic set of authoring documents and previews. The
    // gallery also contains four more scenes that users can open at runtime.
    for id in ["ribbon-bloom", "wire-flora", "neon-knot", "terrazzo-tile"] {
        let document = scene(id)?;
        let thumbnail = gallery::thumbnail(&document, 480, 300);
        let package = documents.join(format!("{id}.plume"));
        io::save_plume(&package, &document, Some(&thumbnail), &BTreeMap::new())?;
        let preview = previews.join(format!("{id}.png"));
        thumbnail.save(&preview)?;
        println!("wrote {} and {}", package.display(), preview.display());
    }

    // A representative file for every external format PLUME can write.
    let draft = ExportOptions {
        quality: MeshQuality::Draft,
        ..ExportOptions::default()
    };
    io::export_glb(
        &scene("ribbon-bloom")?,
        &interchange.join("ribbon-bloom.glb"),
        &draft,
    )?;
    io::export_obj(
        &scene("terrazzo-tile")?,
        &interchange.join("terrazzo-tile.obj"),
        &draft,
    )?;
    io::export_stl(
        &scene("ribbon-bloom")?,
        &interchange.join("ribbon-bloom.stl"),
        &draft,
        true,
    )?;
    io::export_usdz(
        &scene("ribbon-bloom")?,
        &interchange.join("ribbon-bloom.usdz"),
        &draft,
    )?;

    let feather_document = scene("terrazzo-tile")?;
    let feather_thumbnail = gallery::thumbnail(&feather_document, 480, 300);
    let feather_path = interchange.join("terrazzo-tile.feather");
    io::export_feather(&feather_document, &feather_path, Some(&feather_thumbnail))?;
    println!("wrote interchange samples and {}", feather_path.display());

    write_import_fixtures(&testdata, &draft)?;
    Ok(())
}

fn scene(id: &str) -> Result<Document, Box<dyn Error>> {
    gallery::build_document(id).ok_or_else(|| format!("gallery scene {id} is missing").into())
}

fn write_import_fixtures(testdata: &Path, draft: &ExportOptions) -> Result<(), Box<dyn Error>> {
    let mut document = Document::new("Import Cube");
    let guide_id = document.scene.new_guide_id();
    document
        .scene
        .add_guide(Guide::imported(guide_id, "Cube", cube_mesh(), None));
    let options = ExportOptions {
        quality: draft.quality,
        include_guides: true,
        unit_scale: 1.0,
        ..ExportOptions::default()
    };

    io::export_obj(&document, &testdata.join("import-cube.obj"), &options)?;
    io::export_stl(&document, &testdata.join("import-cube.stl"), &options, true)?;
    io::export_glb(&document, &testdata.join("import-cube.glb"), &options)?;
    println!("wrote import fixtures under {}", testdata.display());
    Ok(())
}

fn cube_mesh() -> Mesh {
    let positions = [
        Vec3::new(-1.0, -1.0, -1.0),
        Vec3::new(1.0, -1.0, -1.0),
        Vec3::new(1.0, 1.0, -1.0),
        Vec3::new(-1.0, 1.0, -1.0),
        Vec3::new(-1.0, -1.0, 1.0),
        Vec3::new(1.0, -1.0, 1.0),
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(-1.0, 1.0, 1.0),
    ];
    let indices = [
        4, 5, 6, 4, 6, 7, // front
        1, 0, 3, 1, 3, 2, // back
        0, 4, 7, 0, 7, 3, // left
        5, 1, 2, 5, 2, 6, // right
        3, 7, 6, 3, 6, 2, // top
        0, 1, 5, 0, 5, 4, // bottom
    ];
    let uvs = positions
        .iter()
        .map(|p| Vec2::new((p.x + 1.0) * 0.5, (p.z + 1.0) * 0.5))
        .collect();
    let mut mesh = Mesh {
        positions: positions.to_vec(),
        normals: Vec::new(),
        uvs,
        indices: indices.to_vec(),
    };
    mesh.compute_smooth_normals();
    mesh
}
