# PLUME engineering decisions

This file records choices made while turning the brief into PLUME. It is the
place to look when you wonder why something is shaped the way it is.

## Product and identity

- **PLUME is an original product.** The brief described a Feather-class 3D
  sketching app. No Feather binary, artwork, font, icon, shader or preset was
  available, and the public repository returned 404. We therefore built the
  engine and studio from first principles and used only platform fonts plus
  procedural vector drawing for identity.
- **The `.feather` format is an interchange format, not a compatibility claim.**
  Feather's native document format is undocumented. PLUME writes a ZIP
  containing a full PLUME document, a documented curve JSON and a README. The
  reader recovers JSON point arrays and OBJ mesh entries on a best-effort basis.
  It does not guess undocumented fields.
- **The product name is PLUME.** The word "Feather" appears only when naming
  file compatibility or the original inspiration.

## Workspace and platform strategy

- **One engine, two shells.** `plume-core` is UI-agnostic Rust. `plume-app` is
  the desktop studio and headless renderer. `plume-ffi` exposes the same
  `plume-core` document model to Kotlin through UniFFI and JNA. Android never
  reimplements stroke capture, extrusion, history or exporters.
- **Desktop rendering is offscreen.** The wgpu renderer draws into HDR, depth,
  shadow and post-processing targets and the finished texture is displayed in
  egui. This gives deterministic screenshots and lets exports run without a
  second rendering path.
- **CPU previews on Android.** The mobile canvas asks the Rust core for a
  camera-aware software preview instead of trying to host a second wgpu surface.
  It is deliberately a fast authoring preview; final surface quality comes from
  the desktop or an exported mesh.
- **ARCore without Sceneform.** The Android AR activity uses ARCore only for
  session, tracking, hit tests and anchors. The camera background, point cloud
  and PLUME meshes are drawn by an original OpenGL ES 2.0 renderer.

## Data and editing

- **Documents are ordinary serialisable data.** `.plume` is a ZIP containing
  `document.json`, an optional `thumbnail.png`, `manifest.json` and `assets/`.
  This keeps schema versioning, recovery and third-party inspection simple.
- **Undo is snapshot-based.** Each gesture records a document snapshot before
  the edit. This avoids a fragile inverse operation for every brush, liquify,
  boolean, mirror and import path. UI slider drags are coalesced so one drag is
  one undo step.
- **History is bounded.** The default depth is 100 snapshots. Snapshots include
  the scene, camera, render settings and animation.
- **Dirty is content, not a timer.** The desktop compares a normalised document
  fingerprint with the last saved/opened baseline. Camera orbit, selection,
  thumbnail, scene revision and the modified timestamp are excluded, so undoing
  back to the saved content clears the marker. An imported OBJ/glTF/STL or
  Feather document has no native baseline and starts unsaved until it is saved
  as a `.plume`.
- **Materials are generated.** Brush swatches, UI icons, the app mark and the
  gallery previews are procedural. No external texture atlas is required.
- **Procedural brushes are shader descriptions plus CPU extrusion.** The brush
  library drives both live drawing and exported meshes, so a saved material
  looks the same after reopening.
- **Mesh quality is explicit.** Draft/Normal/High/Export presets trade sampling
  density for speed. Export uses the densest setting and welds where useful.
- **Mirroring is a scene operation.** Mirror copies are ordinary curves tagged
  as mirrors, so they can be selected, exported or deleted like any other art.
- **Imported art is framed, not rescaled.** The core frames imported bounds
  when it builds a document, and the desktop reframes after importing into an
  open scene. `Camera::frame_bounds` uses `MIN_NEAR` as its radius floor rather
  than one world unit, so metre-scale models are not left as specks. The source
  coordinates themselves are preserved.

## Coordinates and units

- Right-handed, Y-up, XZ ground plane.
- World units are millimetres by convention. Exporters convert to metres unless
  the caller asks for millimetres.
- **Importers keep source coordinates.** OBJ/glTF/STL vertices enter the
  document as PLUME millimetres; there is no silent 1000× conversion. OBJ and
  STL carry no unit metadata, and guessing could misread deliberately small
  artwork. Framing makes an import visible, but its physical scale relative to
  brushes and guides remains ambiguous until an explicit import scale or unit
  selector exists. glTF's metre convention is the clearest candidate for that
  option.
- Cameras use a wgpu-style 0..1 depth projection. The core also exposes the
  equivalent nalgebra projection for tooling and tests.
- The default drawing plane is camera-facing and passes through the selection
  centre, the active guide centre, or the camera target, in that order.

## Safety and reliability

- **One scene and one headless destination per invocation.** `--gallery` and
  `--file` are mutually exclusive rather than one silently winning.
  `--render`, `--output` and `-o` all imply `--headless`, and the destination
  must be a file with a `.png`, `.jpg` or `.gif` extension.
- **Project-local toolchain.** Android verification uses a portable JDK, SDK and
  Gradle under the ignored `.tools/` directory. No system PATH, registry entry
  or service is modified.
- **Archive limits.** Package readers reject path traversal, absolute paths,
  oversized entries and archives that expand beyond 512 MiB.
- **Atomic package writes.** `.plume` saves write and flush a sibling temporary
  file, then rename it over the destination. The old package remains intact if
  the write fails.
- **An approved quit is final.** Closing with unsaved changes cancels the first
  close request and shows Save and continue / Discard changes / Cancel.
  Choosing to discard marks the quit as approved, so the follow-up close is not
  intercepted by the dirty check again; `Escape` and Cancel dismiss the prompt
  and keep the document dirty. A clean document closes without a prompt.
- **Destinations are validated before anything is created.** `Sandbox::ensure_file`
  rejects `..` components and checks the nearest existing ancestor against the
  sandbox before calling `create_dir_all`, then re-checks the canonical parent.
  A refused write must not leave folders outside the boundary. The project
  folder (`local_projects`) is created on first file-dialog use, not at launch,
  so native dialogs open there instead of silently falling back to Documents.
- **Frame-all includes visible guides.** `Home` frames every visible curve and
  guide, so a large stage or construction plan is part of the view. `F` frames
  the selection, and hiding a guide removes it from the frame. This follows the
  scene-as-authored model: framing silently ignored guides would make some
  canvases unreachable.
- **No network permission on Android.** The app performs no analytics, uploads
  or remote fetching. AR camera frames stay on the device.
- **Optional FFmpeg.** MP4 capture uses a user-provided `ffmpeg` binary or the
  `PLUME_FFMPEG` override. FFmpeg is not redistributed.
- **Hardware fallback.** The desktop honours `PLUME_SOFTWARE=1` to choose a
  software adapter. This is required on the Intel UHD 620 host where device
  creation with the hardware adapter crashes the driver.

## Scope boundaries

- Desktop is the complete studio: surface and render settings, booleans,
  advanced guides, GIF/MP4 capture and print-grade stills.
- Android prioritises touch authoring, brush/scene browsing, camera-shot
  previews, SAF export, autosave and AR placement. It intentionally does not
  import arbitrary external OBJ/glTF/STL/`.plume` documents through its UI.
- USDZ export is implemented with an original ZIP/USDZ writer. There is no
  USDZ importer because the brief only required export and because USDZ
  validation needs a USD runtime.
- Feather import is best-effort by design. It never silently drops recovered
  OBJ geometry: each OBJ entry becomes an imported guide.

## Verification decisions

- Unit tests live with each core module. FFI tests exercise the bridge the way
  Kotlin uses it. Desktop tests avoid requiring a GPU.
- Headless screenshot tests use the WARP/software adapter when available.
- Android verification is `testDebugUnitTest`, `lintDebug` and `assembleDebug`
  using the checked-in Gradle wrapper and generated UniFFI Kotlin.
- Sample assets are generated by PLUME itself, so they double as exporter tests
  and never carry unclear provenance.
