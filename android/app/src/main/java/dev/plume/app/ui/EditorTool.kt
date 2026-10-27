package dev.plume.app.ui

enum class EditorTool(val label: String, val hint: String) {
    Draw("Draw", "One finger draws · two fingers orbit and pinch · three fingers pan"),
    Erase("Erase", "Tap a stroke to remove it"),
    Orbit("Orbit", "Drag to orbit · pinch to zoom · three fingers pan"),
}
