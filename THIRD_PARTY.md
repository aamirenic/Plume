# Third-party software

PLUME's own source is MIT licensed. This page lists the third-party components
that are compiled into or required by the shipped applications. It is not a
replacement for the license files distributed with each dependency.

PLUME does not redistribute Feather code, assets, fonts or binaries. Platform UI
fonts are supplied by the operating system and are not bundled.

## Rust workspace — direct dependencies

| Crate | Used by | License |
| --- | --- | --- |
| `glam` | core, app, ffi | MIT OR Apache-2.0 |
| `nalgebra` | core | Apache-2.0 |
| `serde`, `serde_json` | core, app, ffi | MIT OR Apache-2.0 |
| `thiserror` | core, ffi | MIT OR Apache-2.0 |
| `anyhow` | app | MIT OR Apache-2.0 |
| `image` | core, app, ffi | MIT OR Apache-2.0 |
| `zip` | core | MIT |
| `zstd` | core | MIT |
| `flate2` | core | MIT OR Apache-2.0 |
| `base64` | core | MIT OR Apache-2.0 |
| `gltf` | core | MIT OR Apache-2.0 |
| `obj` | core | Apache-2.0 |
| `stl` | core | BSD-3-Clause |
| `approx` | core | Apache-2.0 |
| `bytemuck` | core, app | Zlib OR Apache-2.0 OR MIT |
| `tokio` | core | MIT |
| `log` | core, app | MIT OR Apache-2.0 |
| `tempfile` | core tests | MIT OR Apache-2.0 |
| `eframe`, `egui`, `egui-wgpu` | app | MIT OR Apache-2.0 |
| `wgpu` | app | MIT OR Apache-2.0 |
| `rfd` | app | MIT |
| `pollster` | app | Apache-2.0 OR MIT |
| `env_logger` | app | MIT OR Apache-2.0 |
| `uniffi` / `uniffi_bindgen` | ffi | MPL-2.0 |

Cargo resolves the full transitive set in [`Cargo.lock`](Cargo.lock). The
lockfile version plus the dependency license metadata is the authoritative
inventory for a release build:

```bash
cargo metadata --format-version 1 | \
  jq '.packages[] | select(.source != null) | {name, version, license}'
```

On Windows PowerShell, use `ConvertFrom-Json` or inspect `cargo deny`/`cargo
about` output if you add a license-report step to CI.

## Android — direct dependencies

| Component | Version | License |
| --- | --- | --- |
| AndroidX Compose BOM and Compose libraries | 2024.12.01 | Apache-2.0 |
| `androidx.activity:activity-compose`, `activity-ktx` | 1.9.3 | Apache-2.0 |
| `androidx.core:core-ktx` | 1.15.0 | Apache-2.0 |
| `androidx.lifecycle` runtime/viewmodel Compose | 2.8.7 | Apache-2.0 |
| Kotlin standard library and Compose compiler plugin | 2.0.21 | Apache-2.0 |
| `kotlinx-coroutines-android` | 1.9.0 | Apache-2.0 |
| Google ARCore SDK (`com.google.ar:core`) | 1.47.0 | Apache-2.0 (SDK); runtime service subject to Google ARCore terms |
| JNA (`net.java.dev.jna:jna`) | 5.14.0 | LGPL-2.1 OR Apache-2.0 |
| JUnit 4 | 4.13.2 | EPL-1.0 (test only) |
| Gradle wrapper / Android Gradle Plugin | 8.9 wrapper, AGP 8.7.3 | Apache-2.0 |

The Gradle dependency graph is resolved at build time. Use
`./gradlew :app:dependencies` from `android/` for the exact transitive set.

## Toolchain and optional tools

| Tool | Role | Distribution |
| --- | --- | --- |
| Rust stable | Compiler and standard library | MIT OR Apache-2.0 |
| JDK 17 | Android build only | Not redistributed; GPLv2 with Classpath Exception |
| Android SDK / NDK | Build only | Not redistributed; Google SDK terms |
| Gradle wrapper | Build only | Apache-2.0 |
| FFmpeg | Optional MP4 encoding | Not redistributed; license depends on the build (LGPL/GPL) |

The portable toolchain under `.tools/` is local verification scaffolding. It is
ignored by Git and is not part of a PLUME source release or APK.

## Platform services

- Android system fonts, windowing and file picker are platform services.
- Google Play Services for AR is installed by the user on supported devices.
- No web service, analytics SDK, crash reporter or ad library is included.

## License texts

Each crate retains its license file in the Cargo registry source tree; each
Android library retains its notices in the resolved AAR. A binary distribution
should include this page, the PLUME [LICENSE](LICENSE) and the applicable
dependency notices. If you add a dependency or change a feature, re-run the
license inventory before shipping.
