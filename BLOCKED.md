# PLUME blockers and verification gaps

This file records what could not be verified in the build environment, why it
matters, and what would close the gap. It is deliberately separate from
`DECISIONS.md` (choices) and `PLAYTEST.md` (what was driven). A gap is listed
here only when it could change a release claim.

## Open — runtime and hardware

### 1. No Android device or emulator

- **Status:** open, environmental.
- **Impact:** the APK is compile/lint/package verified but has never been
  installed. Touch stroke capture, pressure, multi-finger orbit/pinch/pan,
  autosave restore, SAF export, JNA loading of `libplume_ffi.so`, and ARCore
  placement are unverified at runtime.
- **Why it is blocked:** this host has no Android device attached, and no
  emulator system image or hardware acceleration was provisioned. The Android
  SDK contains build tools only.
- **What would close it:** install `app-debug.apk` on an API 26+ arm64 device
  or an x86_64 emulator with Google Play services for AR, then run the editor
  and AR smoke flows in `PLAYTEST.md`.
- **Work already done:** both ABI bridges are packaged and were listed from the
  APK; `:app:testDebugUnitTest`, `:app:lintDebug` and `:app:assembleDebug`
  pass; lint has 0 errors.

### 2. Hardware wgpu device creation crashes on this GPU

- **Status:** open, host-specific.
- **Impact:** every GUI and headless verification used the Microsoft Basic
  Render Driver (software) instead of the Intel UHD 620 hardware adapter.
  Shader correctness and the post stack are exercised, but hardware-specific
  driver behaviour and frame time are not.
- **Why it is blocked:** creating a wgpu device on the Intel adapter terminates
  the process on this machine. This reproduces outside PLUME's own code.
- **Workaround:** `PLUME_SOFTWARE=1` for the editor; the headless path selects a
  usable adapter by default.
- **What would close it:** run the same captures on a machine where the Intel
  or a discrete GPU initialises successfully, and compare output and timing.

### 3. No macOS or Linux hardware

- **Status:** open, environmental.
- **Impact:** the Rust workspace is platform-neutral and wgpu/eframe are
  configured for Metal/GLES, but only Windows was compiled and run. Native
  dialogs, window chrome, font fallback and filesystem behaviour on macOS and
  Linux are unverified.
- **What would close it:** `cargo test --workspace`, `cargo clippy`, a release
  build and a short GUI smoke run on each target OS.

## Open — test harness

### 4. Native Open/Save As dialogs cannot be driven reliably

- **Status:** open, automation-only.
- **Impact:** Save to an existing `.plume` path was driven end to end, but the
  native Open, Save As and Import Mesh dialogs were not. Their cancellation
  logic is covered by code review and the dirty-state tests, not by a scripted
  run.
- **Why it is blocked:** `rfd` dialogs are separate native windows. In this
  session `SetForegroundWindow` is unreliable, the dialog can open behind the
  main window, and synthetic keyboard input can land in the wrong process.
- **What would close it:** a UI automation driver with real window-level input
  (for example WinAppDriver) or a manual pass on a normal desktop.

### 5. 720p display leaves little room for the default window

- **Status:** open, cosmetic/environmental.
- **Impact:** the default 1280×680 client plus window chrome is tight on a
  1280×720 screen; the status strip can be clipped until the user resizes or
  maximises. The layout itself reflows correctly.
- **What would close it:** verify on a 1280×720 screen and, if desired, lower
  the default inner height or start maximised on small work areas.

## Open — external references

### 6. No Feather application or document format

- **Status:** open, external.
- **Impact:** `.feather` support is a documented best-effort interchange
  format, not native Feather compatibility. Brush dynamics, hidden layers and
  native guide metadata may not transfer.
- **Why it is blocked:** `https://github.com/paper-rockets/Feather3D` returns
  404 and no Feather binary, document or specification was supplied. Guessing
  undocumented fields would be worse than stating the limit.
- **What would close it:** a lawful sample document and a published format
  description; then add fixtures and round-trip tests.

### 7. USDZ has no local validator

- **Status:** open, tooling.
- **Impact:** USDZ is written from the published ZIP/USDA rules and covered by
  structural tests, but it has not been opened in a USD-capable DCC or viewer.
- **Why it is blocked:** no USD runtime is installed, and PLUME intentionally
  does not bundle one.
- **What would close it:** open the generated `samples/interchange/ribbon-bloom.usdz`
  in usdview, Blender, or another USD consumer and record the result.

### 8. Release signing and store distribution

- **Status:** open, credentials.
- **Impact:** only a debug APK exists. `assembleRelease` has minification
  configured but no signing config or upload key. There is no Play Console
  listing, privacy policy, screenshots set or content rating.
- **What would close it:** create a release keystore outside the repository,
  configure signing in a local (ignored) properties file, produce an AAB,
  complete the store listing and run the Play pre-launch report.

## Resolved in this pass

These were blockers during the build and are now fixed; they are kept here so a
future reader does not re-open them by mistake.

- Native Android bridges were missing from the APK. They are built with
  `cargo-ndk` for `arm64-v8a` and `x86_64` and packaged.
- `ArActivity.kt` was missing the generated `R` import.
- Dirty state survived undo and was cleared by a cancelled Open.
- Imported mesh/Feather documents claimed to be saved without a `.plume` file.
- Corrupt packages and unknown gallery ids produced dead-end messages.
- Zero/oversized render dimensions were silently clamped.
- Terrazzo Tile ignored its band colours at load time.
- CLI help lost its indentation, and missing-file errors duplicated the OS
  reason.

## Non-blocking scope decisions

These are intentional and documented in `DECISIONS.md`; they are not release
blockers for the stated product.

- Android exposes touch authoring, export, gallery, animation preview and AR,
  but not the full desktop surface/import control set.
- Full OBJ/glTF/STL/`.plume` import is not exposed on Android.
- Importers preserve source coordinates as PLUME millimetres; they do not
  rescale metre-authored OBJ/STL files or apply glTF's metre convention.
  Automatic framing keeps imports visible, but physical scale relative to
  brushes and guides remains ambiguous until an explicit import scale or unit
  selector exists.
- MP4 export requires a local `ffmpeg`; it is not redistributed. FFmpeg 9.0.2
  was present on this host and the core MP4 test passed; the GUI encode job was
  not driven.
- PLUME does not install anything, change PATH/registry/services, request
  network access or include analytics.
