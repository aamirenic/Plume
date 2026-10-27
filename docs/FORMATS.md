# PLUME file formats

This document describes what PLUME writes and reads. The authoritative schema is
the serde model in [`core/src/scene.rs`](../core/src/scene.rs); this page is the
human-readable contract.

## Units and coordinates

- Right-handed, **Y up**, ground plane on XZ.
- World units are millimetres by convention.
- The renderer maps one world unit to one millimetre.
- Exporters default to metres (`unit_scale = 0.001`) for OBJ, glTF and USDZ,
  because those formats usually assume metres in a DCC.
- STL keeps the caller's scale too; the CLI and samples use metres.
- Importers keep vertex coordinates as PLUME units; no unit conversion is
  applied. OBJ and STL carry no unit metadata, and glTF's metre convention is
  not converted on import. Imported documents are framed to their bounds, but
  physical scale relative to brushes and guides must be accounted for by the
  artist until an import-scale option exists.
- Cameras use a wgpu-style 0..1 depth projection. The core also exposes the
  equivalent nalgebra projection for tooling and tests.

Mesh quality is an explicit enum:

| Quality | Intended use |
| --- | --- |
| `Draft` | Touch previews, AR and fast iteration |
| `Normal` | Desktop viewport |
| `High` | Stills and hero captures |
| `Export` | Final mesh export; densest sampling and welding |

## `.plume` document package

A `.plume` file is a ZIP archive. All paths are relative and validated.

```text
scene.plume
├── document.json      required, UTF-8 JSON
├── thumbnail.png      optional, RGBA PNG
├── manifest.json      optional, package metadata
└── assets/            optional, additional named byte blobs
    └── ...
```

### `manifest.json`

```json
{
  "schema": "plume.package",
  "version": 1,
  "application": "PLUME",
  "document": "document.json",
  "thumbnail": "thumbnail.png",
  "assets": ["tiles/paper.png"],
  "created": "2026-10-27T00:00:00Z"
}
```

The reader tolerates a missing manifest and reports that it recovered
`document.json`. A missing `document.json` or a foreign `schema` is a hard error.

### `document.json`

The document schema is `plume.document`, format version `1`.

Top-level fields:

| Field | Meaning |
| --- | --- |
| `schema`, `version` | Format identity and version |
| `name`, `author`, `notes` | Human metadata |
| `created`, `modified` | UTC timestamps (ISO 8601 text) |
| `camera` | Position, target, up, FOV, near/far, roll |
| `render` | Background, environment, lighting, grid and post settings |
| `scene` | Curves, guides, groups, selection, mirror settings and ID counters |
| `animation` | Timeline, turntable, camera shots and keyframes |

The scene contains authored data only. GPU buffers, BVH nodes, extruded meshes
and guide caches are rebuilt after loading, so files stay small and portable.
Curves store control points with pressure, Catmull-Rom/circle/line kind, frame
data, colour, visibility and a full brush description. Brushes include the
cross-section profile, dynamics, taper and procedural pattern parameters.

### Save behaviour

- The writer strips the in-memory thumbnail, updates `modified`, and writes
  pretty JSON.
- The package is serialised in memory first, then written to a sibling
  `*.plume.<pid>.tmp` file. The file is flushed with `sync_all` and renamed over
  the destination. A failed write leaves the previous package intact.
- Thumbnails are encoded as PNG.
- Asset names are sanitised and stored under `assets/`.

### Safety limits

The reader rejects:

- names containing `..`, backslashes or a leading `/`
- entries larger than 128 MiB
- archives whose declared contents exceed 512 MiB

The reader also refuses a document whose schema is not `plume.document` or whose
format version is newer than the build supports.

## `.feather` interchange

Feather's native document format is not publicly documented. PLUME does **not**
claim byte-level compatibility.

### Export

A PLUME `.feather` file is a ZIP archive containing:

```text
scene.feather
├── document.json    full PLUME document
├── curves.json      documented point arrays
├── thumbnail.png    optional
└── README.txt       provenance and usage note
```

`curves.json` has this shape:

```json
{
  "curves": [
    {
      "name": "Ribbon 1",
      "closed": false,
      "kind": "catmullrom",
      "color": [0.97, 0.43, 0.31, 1.0],
      "points": [
        { "position": [0.0, 0.0, 0.0], "pressure": 0.85 }
      ]
    }
  ]
}
```

### Import

The reader walks ZIP entries and recovers:

- JSON objects containing `curves`, `strokes`, `paths`, `lines` or `splines`
  arrays, or objects with `points` / `controlPoints` / `control_points`
- OBJ entries, converted to imported mesh guides

Duplicate point sequences are removed. Unknown entries and oversized entries are
reported and skipped. If nothing recognisable is found, import fails with an
explanation and suggests exporting OBJ or glTF from the source application.
Imported Feather geometry never silently becomes the active drawing guide.

## OBJ / MTL

- Export writes one object per visible curve (and optionally per visible guide),
  with positions, UVs, normals and triangle faces.
- Materials are written to a sibling `.mtl` file; diffuse colour and emission are
  preserved where the material model allows.
- `ExportOptions::merge` can collapse everything into one object.
- Import reads OBJ geometry and material colour and creates imported mesh guides.

## glTF / GLB

- Export writes glTF 2.0 with separate objects, triangle primitives, normals,
  UVs, PBR base colour, alpha blending where needed and emissive factors.
- Emissive strength above 1 uses `KHR_materials_emissive_strength`.
- `.glb` embeds the binary buffer; `.gltf` writes external buffer files as
  needed.
- Import reads scene meshes, transforms, material base colours and names, and
  converts them to imported guides.

## STL

- Export always merges to a single mesh because STL has no object hierarchy.
- Binary STL is written by default; the core API also exposes ASCII output.
- Import supports binary and ASCII STL.

## USDZ

PLUME writes USDZ directly from the published ZIP and USDA specifications:

- uncompressed ZIP entries
- local file data aligned to 64 bytes
- an ASCII USDA mesh hierarchy with normals and UVs
- `metersPerUnit = 1`, `upAxis = "Y"`

There is no USDZ importer; validating USDZ fully requires a USD runtime, which
PLUME does not bundle.

## Images and animation

- PNG: single still, rendered on the desktop offscreen renderer.
- GIF: frame sequence with a fixed palette and configurable delay.
- MP4: rendered frames are piped to a local `ffmpeg` executable. PLUME does not
  redistribute FFmpeg. Set `PLUME_FFMPEG` or put `ffmpeg` on `PATH`.
- MP4 capture is intentionally a local-tool integration; failure to find ffmpeg
  is reported as an export error, not a crash.

## Round-trip expectations

| Format | Import | Export | Round-trip fidelity |
| --- | --- | --- | --- |
| `.plume` | Yes | Yes | Full authored document |
| `.feather` | Best effort | Yes (interchange) | Geometry and colours; not native Feather state |
| OBJ/MTL | Yes | Yes | Geometry, normals, UVs, diffuse colour |
| glTF/GLB | Yes | Yes | Geometry, normals, UVs, PBR base colour, emission |
| STL | Yes | Yes | Geometry only; merged |
| USDZ | No | Yes | Geometry, normals, UVs, colour |
| PNG/GIF/MP4 | N/A | Yes | Rendered media, not editable documents |

## Regenerating the samples

Every committed sample and fixture is produced by PLUME itself:

```bash
cargo run -p plume-core --example generate_samples
```

The generator writes the four authoring samples, their previews, one file per
interchange format and the import fixtures under `testdata/`. It is deterministic
apart from timestamps embedded in the documents.
