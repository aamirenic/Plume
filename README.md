# PLUME

**PLUME is a 3D curve studio.** Draw pressure-aware strokes in space, wrap them
in procedural materials, build on construction surfaces, light the result like a
product shot, and export a model, still or turntable.

The repository contains two shells over one Rust engine:

- **PLUME Studio** — a Rust/wgpu desktop editor with an offscreen HDR renderer,
  33 brushes, guides, liquify, groups, animation and model/media export.
- **PLUME for Android** — a Kotlin/Jetpack Compose touch editor and ARCore
  placement view backed by the same `plume-core` document model through UniFFI.

PLUME is an original, clean-room implementation. It does not contain Feather
artwork, icons, fonts, binaries or presets. See [NOTICE.md](NOTICE.md).

![PLUME desktop studio showing the Ribbon Bloom gallery scene](docs/renders/desktop-studio.png)

## Highlights

- **Curves that behave like ribbons and tubes.** Catmull-Rom and circular
  strokes, rotation-minimising frames, pressure taper, round/flat/custom
  cross-sections and adaptive extrusion.
- **33 procedural brushes** across sketch, marker, calligraphy, ink, hatch,
  halftone, terrazzo, stipple, smooth, rough, taper and experimental families.
  Patterns are shader descriptions plus CPU sampling, not copied texture atlases.
- **Guides and picking.** Plane, box, sphere, cylinder, cone, torus, freeform,
  loft and imported-mesh guides; BVH-backed selection, surface snapping and
  marquee picking.
- **Direct editing.** Brush, erase, liquify (push/pull/twirl/pinch), select,
  move, line, circle, ellipse and polygon tools; duplicate, groups, mirror,
  visibility and locking.
- **A real render.** HDR colour, depth, shadow map, studio environment, up to
  three-point lighting, rim/specular response and a post stack with ACES tone
  mapping, bloom, vignette, grain, chromatic aberration, sharpen, fog, depth of
  field and outlines.
- **Animation.** Turntable playback, camera keyframes, camera shots, timeline
  scrubbing and GIF/MP4 capture.
- **Interchange.** Native `.plume` packages, best-effort `.feather` interchange,
  OBJ/MTL, glTF/GLB, STL and USDZ export; OBJ/glTF/STL import; PNG/GIF/MP4 stills
  and animation.
- **Mobile authoring.** Touch drawing with pressure, orbit/pinch/pan, gallery,
  brush browser, undo/redo, animation scrubbing, autosave, SAF export and ARCore
  tabletop placement without Sceneform.

## Screens

| Studio | Gallery |
| --- | --- |
| ![PLUME desktop studio](docs/renders/desktop-studio.png) | ![PLUME scene gallery](docs/renders/scene-gallery.png) |

All eight built-in scenes are rendered by the headless renderer in
[`docs/renders/`](docs/renders/). They are generated from PLUME curves and
brushes, not from third-party artwork.

## Quick start

Requirements:

- Rust stable with `rustfmt` and `clippy` (pinned by `rust-toolchain.toml`)
- A Vulkan, DX12, Metal or GLES 3 adapter; a software adapter is also supported
- For Android: JDK 17, Android SDK 35, NDK 27.2 and the Rust Android targets

```bash
# Build and run the desktop studio
cargo run -p plume-app --release

# Run the full Rust test suite
cargo test --workspace

# Lint the workspace
cargo clippy --workspace --all-targets -- -D warnings

# Regenerate committed samples, previews and import fixtures
cargo run -p plume-core --example generate_samples
```

### Headless capture

```bash
# Render a gallery scene
cargo run -p plume-app -- --gallery ribbon-bloom \
  --render ribbon.png --width 2400 --height 1500 --quality export

# Render an existing document
cargo run -p plume-app -- --file samples/documents/neon-knot.plume \
  --render neon.png --quality high

# Open the studio on a specific scene or document
cargo run -p plume-app --release -- --gallery wire-flora
cargo run -p plume-app --release -- samples/documents/terrazzo-tile.plume
```

Relative paths are resolved inside the PLUME sandbox, not the shell's current
directory. The default sandbox is the parent of the Cargo workspace; point it at
the repository when you want repo-local output paths:

```bash
PLUME_SANDBOX="$PWD" cargo run -p plume-app -- \
  --gallery ribbon-bloom --render docs/renders/ribbon-bloom.png
```

On PowerShell:

```powershell
$env:PLUME_SANDBOX = (Get-Location).Path
cargo run -p plume-app -- --gallery ribbon-bloom --render docs/renders/ribbon-bloom.png
```

On hosts whose hardware graphics driver crashes during wgpu device creation,
select the software adapter explicitly:

```bash
PLUME_SOFTWARE=1 cargo run -p plume-app --release
```

## Controls

| Input | Action |
| --- | --- |
| Right-drag / Alt+left-drag | Orbit |
| Shift+right-drag / middle-drag | Pan |
| Wheel | Zoom |
| B / E / L | Brush, erase, liquify |
| V / T / G | Select, move, guide |
| C | Straight line |
| 1 / 2 / 3 / 4 | Perspective, top, front, right |
| F / Home | Frame selection / frame scene |
| `[` / `]` | Brush size down / up |
| Space | Play or pause animation |
| Ctrl+Z / Ctrl+Y | Undo / redo |
| Ctrl+S / Ctrl+Shift+S | Save / save as |
| Ctrl+O / Ctrl+N | Open / new |
| Ctrl+D / Ctrl+G / Ctrl+Shift+G | Duplicate / group / ungroup |
| Delete | Delete selection |

The in-app **Help → Quick start and shortcuts** dialog mirrors this list.

## Files and safety

- `.plume` is a ZIP package with `document.json`, an optional `thumbnail.png`,
  `manifest.json` and optional `assets/`. Saves are atomic: PLUME writes and
  flushes a sibling temporary file, then renames it over the destination.
- `.feather` is a documented PLUME interchange archive, not a claim of native
  Feather compatibility. Import recovers JSON point arrays and OBJ entries on a
  best-effort basis and reports what it skipped.
- Every read and write is constrained to a sandbox. Archive readers reject path
  traversal, absolute paths, oversized entries and archives that expand beyond
  512 MiB.
- The desktop app installs nothing and changes no PATH, registry key or service.
  The Android app requests no network permission and performs no analytics.
- MP4 output uses a local `ffmpeg` executable (or `PLUME_FFMPEG`); FFmpeg is not
  redistributed.

See [docs/FORMATS.md](docs/FORMATS.md) for the document and interchange details.

## Repository map

```text
core/       UI-agnostic Rust engine: curves, RMF, guides, BVH, extrusion,
            brushes/patterns, liquify, animation, render data, import/export
app/        Desktop studio (eframe + wgpu) and headless renderer
ffi/        UniFFI bridge consumed by the Android shell
android/    Kotlin/Compose app, ARCore activity and Gradle project
samples/    Generated .plume documents, previews and interchange files
testdata/   Generated OBJ/glTF/STL import fixtures
docs/       Format reference and curated QA renders
```

## Verification

The current verified state is:

- `cargo test --workspace`: **206 tests pass** (184 core, 14 desktop, 8 FFI)
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo fmt --all -- --check`: clean
- Android `testDebugUnitTest`, `lintDebug` and `assembleDebug`: **BUILD
  SUCCESSFUL**, 0 lint errors
- APK: `arm64-v8a` and `x86_64` native bridges packaged
- All eight gallery scenes rendered headlessly and visually inspected

Android has been compile/lint/package verified but not run on a physical device
in this environment. See [BLOCKED.md](BLOCKED.md) for the remaining
verification gaps, [PRODUCTION.md](PRODUCTION.md) for the release checklist and
[PLAYTEST.md](PLAYTEST.md) for the playtest record.

## Android

```bash
# Native bridge (once per ABI)
rustup target add aarch64-linux-android x86_64-linux-android
export ANDROID_NDK_HOME="$ANDROID_SDK_ROOT/ndk/27.2.12479018"
cargo ndk -t arm64-v8a -t x86_64 \
  -o android/app/src/main/jniLibs build --release -p plume-ffi

# Build
cd android
./gradlew :app:testDebugUnitTest :app:lintDebug :app:assembleDebug
```

Full Android instructions, including UniFFI regeneration and ARCore notes, are
in [android/README.md](android/README.md).

## Documentation

- [DECISIONS.md](DECISIONS.md) — architecture and scope decisions
- [PRODUCTION.md](PRODUCTION.md) — readiness, verification and release checklist
- [PLAYTEST.md](PLAYTEST.md) — what was driven, what broke, what remains
- [BLOCKED.md](BLOCKED.md) — unresolved environment and verification gaps
- [docs/FORMATS.md](docs/FORMATS.md) — document and interchange formats
- [samples/README.md](samples/README.md) — sample provenance and regeneration
- [testdata/README.md](testdata/README.md) — import fixture provenance
- [THIRD_PARTY.md](THIRD_PARTY.md) — third-party software and licenses

## License

PLUME source is released under the MIT license. Third-party dependencies retain
their own licenses. Feather is a name owned by its respective owner and is used
only to describe an optional interchange format.
