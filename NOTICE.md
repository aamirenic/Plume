# Clean-room and attribution notice

PLUME is an independent implementation. It is not affiliated with, endorsed by,
or derived from Feather or any other commercial sketching application.

The following were created for this repository:

- All Rust engine, renderer and file-format code.
- The Kotlin/Jetpack Compose Android application.
- The PLUME wordmark, application icon and all in-app icons.
- The default ink, ivory, coral, teal and gold visual palette.
- The procedural brush library and brush swatches.
- The eight built-in gallery scenes.
- The shaders, test scenes, documentation and sample files.

No Feather application binary, icon, font, image, shader, preset, scene,
trademark or other asset is copied into or redistributed with PLUME. The
`.feather` reader and writer are independent, best-effort interchange code
based only on publicly observable ZIP/JSON/OBJ conventions. They do not claim
byte-level compatibility with an undocumented native Feather format.

The app uses the default fonts provided by the platform UI toolkit. It does not
ship a copied display font. Icons are drawn from arcs, lines and closed paths in
Compose and in the desktop renderer.

## Third-party software

PLUME depends on open-source crates and Android libraries. Their licenses remain
in force and are listed in `THIRD_PARTY.md`. Cargo's `Cargo.lock` and Gradle's
dependency resolution provide the exact versions used by a build.

Feather is a name owned by its respective owner and is used here only to
describe an optional import/export format. PLUME is the product name.

If you believe a file in this repository infringes your rights, please open an
issue with the file path and supporting evidence. Provenance takes priority
over convenience: questionable files will be removed rather than rationalised.
