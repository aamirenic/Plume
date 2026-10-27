package dev.plume.app

import android.app.Application
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import dev.plume.app.ui.EditorTool
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.plume_ffi.BrushEntry
import uniffi.plume_ffi.DocumentInfo
import uniffi.plume_ffi.GalleryEntry
import uniffi.plume_ffi.PlumeSession
import java.io.File
import java.util.Locale
import kotlin.math.max
import kotlin.math.min

data class EditorState(
    val info: DocumentInfo? = null,
    val preview: Bitmap? = null,
    val brushes: List<BrushEntry> = emptyList(),
    val gallery: List<GalleryEntry> = emptyList(),
    val galleryThumbs: Map<String, Bitmap> = emptyMap(),
    val selectedBrush: String = "",
    val tool: EditorTool = EditorTool.Draw,
    val animationDuration: Double = 8.0,
    val status: String = "Opening your studio…",
    val busy: Boolean = true,
    val autosavedAt: Long = 0L,
)

/**
 * Single owner of the Rust session.  Geometry, undo history, projection and
 * export all live in `plume-core`; this class only marshals input and keeps a
 * Compose-friendly snapshot of the session.
 */
class PlumeViewModel(application: Application) : AndroidViewModel(application) {
    private val session = PlumeSession()

    private val autosaveFile = File(application.filesDir, "autosave.plume.json")
    private var previewJob: Job? = null
    private var autosaveJob: Job? = null
    private var previewWidth = 720
    private var previewHeight = 540
    private var strokeActive = false
    private var strokePoints = 0

    private val _ui = MutableStateFlow(EditorState())
    val ui: StateFlow<EditorState> = _ui.asStateFlow()

    init {
        viewModelScope.launch(Dispatchers.Default) {
            val restored = if (autosaveFile.isFile) {
                runCatching {
                    session.loadDocumentJson(autosaveFile.readText())
                }.getOrNull()
            } else {
                null
            }
            if (restored == null) {
                runCatching { session.loadGallery("ribbon-bloom") }
                    .onFailure { failure ->
                        _ui.update { it.copy(status = "Could not open the demo scene: ${failure.message}") }
                    }
            }
            val info = runCatching { session.documentInfo() }.getOrNull()
            val brushes = runCatching { session.brushes() }.getOrDefault(emptyList())
            val gallery = session.gallery()
            val duration = runCatching { session.animationDuration() }.getOrDefault(8.0)

            val thumbs = buildMap {
                for (entry in gallery) {
                    val png = runCatching {
                        session.galleryPreviewPng(entry.id, 360u, 225u)
                    }.getOrNull() ?: continue
                    val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size) ?: continue
                    put(entry.id, bitmap)
                }
            }

            _ui.value = EditorState(
                info = info,
                brushes = brushes,
                gallery = gallery,
                galleryThumbs = thumbs,
                selectedBrush = info?.brushId ?: brushes.firstOrNull()?.id.orEmpty(),
                animationDuration = duration,
                status = if (restored != null) "Restored your last sketch" else "Ribbon Bloom is ready",
                busy = false,
                autosavedAt = if (restored != null) autosaveFile.lastModified() else 0L,
            )
            session.setViewport(previewWidth.toFloat(), previewHeight.toFloat())
            requestPreview(0)
        }
    }

    // -- viewport and preview -------------------------------------------------

    /** Points are the canvas size in pixels; the Rust projection uses the same. */
    fun updateViewport(width: Int, height: Int) {
        if (width < 48 || height < 48) return
        runCatching { session.setViewport(width.toFloat(), height.toFloat()) }
        val scale = min(1.0f, 900.0f / max(width, height).toFloat())
        val newWidth = (width * scale).toInt().coerceIn(160, 900)
        val newHeight = (height * scale).toInt().coerceIn(120, 900)
        if (newWidth != previewWidth || newHeight != previewHeight) {
            previewWidth = newWidth
            previewHeight = newHeight
        }
        requestPreview(40)
    }

    fun requestPreview(delayMs: Long = 70L) {
        val width = previewWidth
        val height = previewHeight
        previewJob?.cancel()
        previewJob = viewModelScope.launch(Dispatchers.Default) {
            if (delayMs > 0) delay(delayMs)
            ensureActive()
            val png = runCatching {
                session.previewPng(width.toUInt(), height.toUInt())
            }.getOrNull() ?: return@launch
            val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size) ?: return@launch
            ensureActive()
            _ui.update { it.copy(preview = bitmap) }
        }
    }

    // -- tools and strokes ----------------------------------------------------

    fun setTool(tool: EditorTool) {
        _ui.update { it.copy(tool = tool) }
        if (strokeActive) {
            cancelStroke()
        }
    }

    fun setBrush(id: String) {
        if (runCatching { session.setBrush(id) }.getOrDefault(false)) {
            _ui.update { it.copy(selectedBrush = id, status = "Brush selected") }
        }
    }

    fun beginStroke(x: Float, y: Float, pressure: Float) {
        runCatching { session.stopPlayhead() }
        strokeActive = true
        strokePoints = 0
        runCatching { session.beginStroke(x, y, pressure.coerceIn(0.05f, 1f)) }
    }

    fun extendStroke(x: Float, y: Float, pressure: Float) {
        if (!strokeActive) return
        strokePoints += 1
        runCatching { session.extendStroke(x, y, pressure.coerceIn(0.05f, 1f)) }
        if (strokePoints % 5 == 0) {
            requestPreview(110)
        }
    }

    fun endStroke() {
        if (!strokeActive) return
        strokeActive = false
        val added = runCatching { session.endStroke() }.getOrDefault(false)
        refreshInfo()
        requestPreview(0)
        if (added) {
            scheduleAutosave()
            _ui.update { it.copy(status = "Stroke added") }
        }
    }

    fun cancelStroke() {
        if (!strokeActive) return
        strokeActive = false
        runCatching { session.cancelStroke() }
        requestPreview(80)
    }

    fun eraseAt(x: Float, y: Float) {
        runCatching { session.stopPlayhead() }
        val erased = runCatching { session.eraseAt(x, y) }.getOrDefault(false)
        if (erased) {
            refreshInfo()
            requestPreview(0)
            scheduleAutosave()
            _ui.update { it.copy(status = "Stroke erased") }
        } else {
            _ui.update { it.copy(status = "No stroke under the finger") }
        }
    }

    // -- camera ---------------------------------------------------------------

    fun orbit(dx: Float, dy: Float) {
        runCatching { session.stopPlayhead() }
        runCatching { session.orbit(dx, dy) }
        requestPreview(90)
    }

    fun pan(dx: Float, dy: Float) {
        runCatching { session.stopPlayhead() }
        runCatching { session.pan(dx, dy) }
        requestPreview(90)
    }

    fun zoom(factor: Float) {
        if (!factor.isFinite() || factor <= 0f) return
        runCatching { session.stopPlayhead() }
        runCatching { session.zoom(factor) }
        requestPreview(90)
    }

    fun frameScene() {
        runCatching { session.stopPlayhead() }
        runCatching { session.frameScene() }
        requestPreview(0)
        scheduleAutosave()
    }

    fun finishCameraGesture() {
        requestPreview(0)
        refreshInfo()
        scheduleAutosave()
    }

    // -- documents ------------------------------------------------------------

    fun newDocument() {
        _ui.update { it.copy(busy = true) }
        viewModelScope.launch(Dispatchers.Default) {
            val result = runCatching { session.newDocument() }
            result.onSuccess { info ->
                _ui.update {
                    it.copy(
                        info = info,
                        busy = false,
                        tool = EditorTool.Draw,
                        status = "New blank scene",
                    )
                }
                requestPreview(0)
                scheduleAutosave()
            }.onFailure { failure ->
                _ui.update { it.copy(busy = false, status = "Could not create a scene: ${failure.message}") }
            }
        }
    }

    fun loadGallery(id: String) {
        _ui.update { it.copy(busy = true) }
        viewModelScope.launch(Dispatchers.Default) {
            runCatching { session.loadGallery(id) }
                .onSuccess { info ->
                    _ui.update {
                        it.copy(
                            info = info,
                            busy = false,
                            tool = EditorTool.Draw,
                            status = "Loaded ${it.gallery.firstOrNull { entry -> entry.id == id }?.name ?: id}",
                        )
                    }
                    requestPreview(0)
                    scheduleAutosave()
                }
                .onFailure { failure ->
                    _ui.update { it.copy(busy = false, status = "Could not load scene: ${failure.message}") }
                }
        }
    }

    fun undo() {
        val changed = runCatching { session.undo() }.getOrDefault(false)
        if (changed) {
            refreshInfo()
            requestPreview(0)
            scheduleAutosave()
            _ui.update { it.copy(status = "Undone") }
        } else {
            _ui.update { it.copy(status = "Nothing to undo") }
        }
    }

    fun redo() {
        val changed = runCatching { session.redo() }.getOrDefault(false)
        if (changed) {
            refreshInfo()
            requestPreview(0)
            scheduleAutosave()
            _ui.update { it.copy(status = "Redone") }
        } else {
            _ui.update { it.copy(status = "Nothing to redo") }
        }
    }

    private fun refreshInfo() {
        val info = runCatching { session.documentInfo() }.getOrNull() ?: return
        _ui.update { it.copy(info = info, selectedBrush = info.brushId) }
    }

    // -- animation ------------------------------------------------------------

    fun scrub(seconds: Double) {
        runCatching { session.setPlayhead(seconds) }
        requestPreview(0)
    }

    fun stopScrub() {
        runCatching { session.stopPlayhead() }
        requestPreview(0)
        refreshInfo()
        scheduleAutosave()
    }

    /** Non-blocking feedback for a gesture or a system action. */
    fun note(message: String) {
        _ui.update { it.copy(status = message) }
    }

    // -- persistence and export ----------------------------------------------

    private fun scheduleAutosave() {
        autosaveJob?.cancel()
        autosaveJob = viewModelScope.launch(Dispatchers.IO) {
            delay(650)
            val json = runCatching { session.documentJson() }.getOrNull() ?: return@launch
            runCatching {
                autosaveFile.parentFile?.mkdirs()
                autosaveFile.writeText(json)
            }.onSuccess {
                _ui.update { it.copy(autosavedAt = System.currentTimeMillis()) }
            }
        }
    }

    /** Synchronous save used from `onStop` so the last edit is never lost. */
    fun saveAutosaveNow() {
        val json = runCatching { session.documentJson() }.getOrNull() ?: return
        runCatching {
            autosaveFile.parentFile?.mkdirs()
            autosaveFile.writeText(json)
        }
    }

    /** Persist before launching AR, then continue on the main thread. */
    fun saveForAr(onReady: () -> Unit) {
        viewModelScope.launch(Dispatchers.IO) {
            saveAutosaveNow()
            withContext(Dispatchers.Main) { onReady() }
        }
    }

    fun export(uri: Uri, format: ExportFormat) {
        _ui.update { it.copy(busy = true, status = "Exporting ${format.label}…") }
        viewModelScope.launch(Dispatchers.IO) {
            val result = runCatching {
                val bytes = if (format == ExportFormat.Png) {
                    session.previewPng(1024u, 768u)
                } else {
                    val cache = File(getApplication<Application>().cacheDir, "exports")
                        .apply { mkdirs() }
                    val target = File(cache, "plume-export.${format.extension}")
                    session.exportDocument(target.absolutePath, format.extension, "export")
                    target.readBytes().also { target.delete() }
                }
                getApplication<Application>().contentResolver
                    .openOutputStream(uri, "w")?.use { stream ->
                    stream.write(bytes)
                } ?: error("the destination could not be opened")
                bytes.size
            }
            result.onSuccess { size ->
                _ui.update {
                    it.copy(
                        busy = false,
                        status = "Exported ${format.label} (${formatSize(size)})",
                    )
                }
                refreshInfo()
            }.onFailure { failure ->
                _ui.update {
                    it.copy(busy = false, status = "Export failed: ${failure.message}")
                }
            }
        }
    }

    private fun formatSize(bytes: Int): String = when {
        bytes >= 1_048_576 -> String.format(Locale.getDefault(), "%.1f MB", bytes / 1_048_576.0)
        bytes >= 1024 -> String.format(Locale.getDefault(), "%.0f KB", bytes / 1024.0)
        else -> "$bytes B"
    }

    override fun onCleared() {
        saveAutosaveNow()
        runCatching { session.close() }
        super.onCleared()
    }
}
