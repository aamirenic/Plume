package dev.plume.app

/** Formats offered by the mobile export sheet. */
enum class ExportFormat(
    val extension: String,
    val mimeType: String,
    val label: String,
    val detail: String,
) {
    Plume("plume", "application/zip", "PLUME document", "Editable scene with preview"),
    Feather("feather", "application/zip", "Feather-compatible document", "Portable curve archive"),
    Glb("glb", "model/gltf-binary", "glTF binary", "Textured-ready mesh for AR and DCC"),
    Gltf("gltf", "model/gltf+json", "glTF 2.0", "Embedded JSON scene"),
    Obj("obj", "text/plain", "Wavefront OBJ", "Widely supported mesh exchange"),
    Stl("stl", "model/stl", "STL", "Binary mesh for printing"),
    Usdz("usdz", "model/vnd.usdz+zip", "USDZ", "Apple AR Quick Look"),
    Png("png", "image/png", "PNG still", "Current camera, 1024 px"),
}
