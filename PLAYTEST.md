# PLUME playtest record

Last pass: 2026-10-28. Build: PLUME 0.1.0, post-release playtest fixes on top
of commit `bd6d1ff` (`PLUME: production ready`). The previous full pass was
2026-10-27.
Host: Windows 11 (10.0.26100), Intel UHD Graphics 620, 1280×720-class
display, Rust 1.99.1, JDK 17.0.20.1, Android SDK 35, NDK 27.2.12479018.

PLUME Studio requires `PLUME_SOFTWARE=1` on this host. The Intel driver crashes
during hardware wgpu device creation; the software adapter is the documented
workaround and was used for every GUI run. Headless rendering uses the same
code path and was also run on the software adapter.

The Android shell has been built, linted and packaged, but no device or
emulator was available. Android runtime claims below stop at the APK boundary.

## 2026-10-28 post-release pass

This pass started from the committed production release and drove the surfaces
a careless user would touch first: imports, command-line mistakes, cancelled or
missing files, and close-with-unsaved-changes.

### What was driven

- `--help`, `--version`, every gallery id, `--output`/`-o`, `--render`,
  `--headless`, and sample-count feedback.
- Missing input file, unsupported image extension, an output path that is a
  folder, and `--gallery` combined with `--file`.
- Every checked-in import fixture and generated interchange sample at 96×96
  and 800×800: `.plume`, `.feather`, OBJ, glTF/GLB and STL.
- The studio launched on a missing file and on `testdata/import-cube.obj`,
  including the dirty close dialog after the import.
- The Android bridge and debug APK were rebuilt from the fixed core, then
  re-tested, re-linted and re-packaged.

### Defects found and fixed

Ordered worst first. Every item was observed through a real command or window,
not inferred from a hypothetical failure mode.

| # | Severity | What a user would hit | Fix | Re-verified by |
| --- | --- | --- | --- | --- |
| 1 | Release blocker | Opening an OBJ, glTF or STL showed only the grid: the imported geometry sat far outside the default camera. The document was editable but looked empty. | `ImportedScene::to_document` now frames the imported bounds. The GUI import path also frames merged bounds after adding a mesh to an open document. | Import matrix at 96×96 and 800×800; the cube fills the frame in the studio instead of disappearing. |
| 2 | Major | After framing, metre-scale imports were still tiny specks because `frame_bounds` forced a minimum radius of one unit. | The radius floor is now `MIN_NEAR` (0.05), so small bounds are framed closely. | Small-import regression test; cube and terrazzo renders at 96×96 (the model fills the frame) and 800×800. |
| 3 | Major | `--output`/`-o` was documented as a headless destination but did not imply `--headless`; the command opened the studio and wrote nothing. | Both aliases now set `headless` exactly like `--render`; help says so. | `--output` and `-o` write 96×96 PNGs and exit 0; unit test covers both. |
| 4 | Misleading input | `--gallery ribbon-bloom --file scene.obj` silently rendered the gallery and ignored the file. | The parser rejects the combination: `--gallery and --file cannot be combined; choose one scene`. | CLI exits 1 with that message; tests cover flag and positional forms. |
| 5 | Misleading startup | `plume --file does-not-exist.obj` silently opened Ribbon Bloom, hiding the typo. | The path is no longer filtered out; the studio opens its fallback scene and reports `Could not open does-not-exist.obj: io: The system cannot find the file specified. (os error 2)`. | GUI screenshot of the status bar while Ribbon Bloom opened. |
| 6 | Misleading error | Rendering to a folder reported a sandbox escape, implying a security boundary problem rather than a bad destination. | `Sandbox::ensure_file` detects an existing folder and says `destination is a folder; choose a file name`. | CLI exits 1 with the folder message; regression test. |
| 7 | Misleading error | An unsupported render extension leaked image-crate text (`The image format Bmp is not supported`). | The CLI validates the extension up front and lists the supported set. | `.xyz`, `.bmp` and an extensionless path exit 1 with `use .png, .jpg or .gif`; test. |
| 8 | Cosmetic | A one-sample render logged `1 samples`. | Render progress now selects `sample` or `samples`. | CLI output shows `1 sample`; plural still used for larger counts. |

### Re-runs after the fixes

- Rust: `cargo fmt --all -- --check` is clean; `cargo test --workspace` passes
  **212 tests** (186 core, 18 desktop, 8 FFI); `cargo clippy --workspace
  --all-targets -- -D warnings` is clean; `cargo build --release -p plume-app`
  succeeds.
- CLI: `--output`, `-o` and `--render` all wrote PNGs; JPEG and GIF also wrote;
  the gallery/file conflict, `.xyz`, extensionless and folder destinations all
  exited 1 with plain-language messages.
- Import matrix: all nine inputs loaded as `.plume`, `.feather`, OBJ, GLB or STL
  at both sizes, exited 0, produced non-empty images, and were visually checked
  for black frames and framing. The checkerboard terrazzo OBJ fills roughly
  half of an 800×800 frame; the unit cube is clearly visible.
- GUI: the missing-file toast appeared while Ribbon Bloom opened; the imported
  cube was framed and titled `PLUME — import-cube •`; closing it opened the
  “Unsaved changes” dialog with Save and continue / Discard changes / Cancel.
- Android: the FFI libraries and debug APK were rebuilt after the core changes;
  `:app:testDebugUnitTest`, `:app:lintDebug` and `:app:assembleDebug` pass with
  0 lint errors. The APK is still device-unverified.

### Residual user friction

These are real but not fixed in this pass.

- **Import units are ambiguous.** PLUME's exporters default to metres, while
  OBJ/STL import treats source coordinates as PLUME millimetres and glTF has no
  unit conversion either. Automatic framing makes a metre-scale import visible,
  but a 1 m source cube becomes a 1 mm scene object. Editing brush sizes and
  liquify radii then relate to the wrong physical scale. A unit selector or a
  documented import scale is the right long-term fix; silently guessing could
  corrupt deliberately small artwork.
- **Native dialogs and automation.** The Windows file dialogs can open behind
  the main window when focus is stolen by a script. Save-to-existing-path was
  driven successfully; Open, Save As and Import Mesh were not driven end to end.
  A user on a busy desktop may need to look for the dialog instead of assuming
  nothing happened.
- **Software rendering latency.** On this host a long stroke can take seconds
  to render. The dirty marker is committed on pointer-up, so a slow frame can
  make “Saved” linger for a moment after the stroke appears. On a hardware
  adapter this is expected to be imperceptible; it is not hidden by the code.
- **Display fit.** The default window is 1280×680 plus chrome. On a 720p
  display placed at the usual 26 px cascade, the status strip is tight or
  partly off-screen until the window is resized or maximised. The window is
  resizable and the layout reflows; a smaller default would suit 720p better.
- **Relative paths are sandbox-relative.** `--render out.png` writes under the
  sandbox root (the workspace parent by default), not the shell's current
  directory. This is deliberate and documented, but it surprises first-time CLI
  users; set `PLUME_SANDBOX=$PWD` or pass an absolute path.
- **No desktop autosave.** The Android shell autosaves to
  `files/autosave.plume.json`; the desktop relies on explicit save and the
  unsaved-changes prompt. A crash loses work since the last save.
- **Import is a one-way document conversion.** OBJ/glTF/STL and `.feather`
  become guides or curves in a new unsaved PLUME document. There is no
  round-trip back to those formats, and Android does not expose full mesh
  import (see `DECISIONS.md`).
- **Android runtime is unverified.** Touch pressure, two/three-finger
  navigation, ARCore placement, SAF export, autosave restore and JNA loading
  are compile/lint/package-verified only. The debug APK has not been installed
  on a device.
- **Feather compatibility is explicitly best effort.** No Feather document or
  specification was available; the reader only recovers documented JSON point
  arrays and OBJ entries.

### Code ownership health

The Rust engine is the healthy part of the tree. `core/src` is 10.1k lines
across 21 domain modules, with the largest files (scene 1108, brush 1001, curve
956, guide 925) still owning a single concept. IO is split per format across
seven files (2.4k lines). Tests live beside the behaviour they cover, and 186
core tests pass. `Document` in `scene.rs` is the single contract shared by the
desktop, Android and every writer; changes there necessarily ripple, but they
are the best-covered changes in the project. The pointer-size and dirty-state
policy correctly live in `app/`, not in core.

The desktop app is where change pressure concentrates. `app/src/ui.rs` is 4.2k
lines and owns every panel, dialog and menu; a change to any panel lands in one
file. `app.rs` (2.6k) owns state, gestures, history and file lifecycle, and
`gpu.rs` (1.9k) owns the renderer and shader upload. The seams are sensible
(UI vs. state vs. GPU vs. sandbox), but `ui.rs` should be split by panel before
it grows further. `theme.rs` and `sandbox.rs` are good small owners: colour and
spacing, and path policy, each have one home.

The Android shell is 3.7k lines of hand-written Kotlin, split by surface
(editor, AR, canvas, sheets, timeline, theme, icons), plus 2.7k lines of
checked-in generated UniFFI bindings. The hand-written side has clear owners;
the generated file should never be edited. The FFI crate is one 813-line
bridge module with its own tests.

The riskiest coupling is desktop dirty/import policy newly living in `app.rs`
while the Android autosave policy lives in `PlumeViewModel.kt`. They share the
document model but not the save lifecycle; a future “save state” refactor
should either move the policy into core or document the two lifecycles
explicitly.

## 2026-10-27 production pass

### What was driven

- `--help`, `--version`, gallery listing, every gallery id, and a headless
  render of all eight scenes.
- Missing input file, empty `.plume`, wrong-schema `.plume`, non-ZIP `.plume`,
  malformed `.obj`, unsupported extension, a path outside the sandbox, an
  unknown gallery id, an unknown quality, and zero/oversized dimensions.
- `--width 16 --height 16` and `--width 7680` bounds; relative and absolute
  render destinations.
- Import fixtures from `testdata/` and the generated samples from `samples/`.
- The studio launched from a sample `.plume`, from the gallery, and from an
  imported OBJ; a stroke was drawn, undone, redone and undone; a sample was
  saved with `Ctrl+S`; a dirty document was closed and the unsaved-changes
  dialog inspected.
- The Android bridge was built for `arm64-v8a` and `x86_64`; unit tests, lint
  and the debug APK were run; the APK manifest, dex and native libraries were
  inspected.

### Defects found and fixed

| # | Severity | What a user would hit | Fix | Re-verified by |
| --- | --- | --- | --- | --- |
| 1 | Release blocker | The first debug APK contained no `libplume_ffi.so`. The Compose UI would fail the first time it called the bridge. | Built the Rust bridge with `cargo-ndk` for both ABIs and reassembled. | `jar tf app-debug.apk` lists `lib/arm64-v8a/libplume_ffi.so` and `lib/x86_64/libplume_ffi.so`; the APK grew from 11.1 MB to over 14.5 MB. |
| 2 | Build blocker | `ArActivity.kt` referenced `R` without importing `dev.plume.app.R`; Kotlin compilation failed. | Added the import. | `BUILD SUCCESSFUL` with 0 lint errors. |
| 3 | Misleading state | After undoing back to the last saved content, the title and status still said “Unsaved”. The marker only cleared on a new save, never on undo. | Dirty is derived from a normalised document fingerprint (content only; camera, selection, thumbnail, revision and timestamp excluded). | GUI title sequence: clean at start → `•` after a stroke → clean after `Ctrl+Z` → `•` after `Ctrl+Y` → clean after `Ctrl+Z`. Unit tests cover the fingerprint and history path. |
| 4 | Data-loss risk | A cancelled Open cleared the dirty flag first, so the next close could discard unsaved work without a prompt. | Removed the unconditional `dirty = false` from `apply_confirm`; each action now sets state only after it succeeds. | Code path reviewed and the dirty-closing dialog was driven (item 5). The native Open dialog itself could not be automated. |
| 5 | Misleading state | An imported OBJ/glTF/STL or Feather document showed “Saved” and closed without warning, although no `.plume` file existed. | Imported documents now have no saved baseline and start dirty; the import is saved as a `.plume` explicitly. | Launching `testdata/import-cube.obj` shows `PLUME — import-cube •`, `Unsaved · Imported 1 OBJ object(s)`; Alt+F4 opens the “Unsaved changes” dialog with Save / Discard / Cancel. |
| 6 | Visual correctness | Terrazzo Tile loaded from `.plume` was monochrome while its gallery preview was colourful, because every band shared the brush's default pattern colour. | Each band's `pattern.color_b` now follows its band colour. | Regression test asserts per-band pattern colour and at least three distinct colours; 1200×750 render matches the preview. |
| 7 | Dead end | `--gallery nope` printed only `unknown gallery scene nope`, with no hint about valid ids. | Shared gallery validation now lists all eight scene ids in headless and GUI modes. | CLI output shows the list; unit test covers the message. |
| 8 | Misleading error | Opening an empty `.plume` printed `zip: invalid Zip archive: Could not find EOCD`; a wrong-schema package printed a raw serde field error. | `.plume` and `.feather` reads now carry plain-language context (“not a readable PLUME package” / “not a readable Feather interchange archive”). | CLI output for empty and wrong-schema files. |
| 9 | Broken bounds | `--width 0 --height 0` silently rendered a 16×16 image and exited successfully; oversized dimensions silently clamped to 7680. | The CLI rejects anything outside 16..=7680 with the offending value. | Unit tests for 0, 7681 and both bounds; CLI exits 1. |
| 10 | Duplicated feedback | A missing file reported the OS reason twice: `io: … (os error 2): … (os error 2)`. | Wrapped errors are rendered into the message but no longer exposed as `Error::source`. | CLI now prints one reason; regression test. |
| 11 | Cosmetic | `--help` lost its two-space indentation because the literal was assembled from continuation lines. | Help is a raw string built by `help_text()`; a test checks indentation. | Help output and test. |

### Re-runs after the production fixes

- Rust: `cargo fmt --all -- --check`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo build --release -p plume-app` all passed. The counts at that time were
  184 core, 14 desktop and 8 FFI tests.
- Android: `:app:testDebugUnitTest :app:lintDebug :app:assembleDebug` was
  `BUILD SUCCESSFUL`; 0 lint errors. The nine warnings were eight “newer
  dependency available” notices and one `ClickableViewAccessibility` warning on
  `ArGLView#onTouchEvent` (the view is a canvas, not a clickable control).
- The dirty/save/close flow was re-run after the fingerprint and import
  changes. The stroke was persisted through `Ctrl+S`, the package on disk grew
  from 5 curves / 35 points to 6 curves / 91 points, and the marker returned to
  “Saved”.
- The Terrazzo scene was regenerated and re-rendered; the preview and render
  now agree.
- All eight scene renders were regenerated after the gallery change and
  inspected for black frames, framing, guide visibility and tone mapping.

## Artifacts

- `docs/renders/` — eight scene renders, studio and gallery captures, and the
  unsaved-changes dialog.
- `.tools/playtest-run/` (screenshots and CLI outputs from the 2026-10-28 pass)
  and all automation scripts were disposable and have been removed. The local
  SDK/JDK toolchain under `.tools/` is ignored by Git.
