package dev.plume.app.ui

import android.app.Activity
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.plume.app.ExportFormat
import dev.plume.app.PlumeViewModel
import dev.plume.app.ar.ArActivity
import java.util.Locale

private enum class EditorSheet { Gallery, Brushes, Export }

@Composable
fun EditorScreen(viewModel: PlumeViewModel) {
    val state by viewModel.ui.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var sheet by remember { mutableStateOf<EditorSheet?>(null) }
    var confirmNew by remember { mutableStateOf(false) }
    var menuOpen by remember { mutableStateOf(false) }
    var stageMenuOpen by remember { mutableStateOf(false) }
    var timelineVisible by remember { mutableStateOf(false) }
    val dismissScrim = remember { MutableInteractionSource() }
    val pendingExport = remember { mutableStateOf<ExportFormat?>(null) }

    val exportLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.StartActivityForResult(),
    ) { result ->
        val format = pendingExport.value
        val uri = result.data?.data
        if (result.resultCode == Activity.RESULT_OK && uri != null && format != null) {
            viewModel.export(uri, format)
        }
        pendingExport.value = null
    }

    fun startExport(format: ExportFormat) {
        pendingExport.value = format
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
            addCategory(Intent.CATEGORY_OPENABLE)
            type = format.mimeType
            putExtra(Intent.EXTRA_TITLE, suggestedName(state.info?.name, format))
        }
        sheet = null
        runCatching { exportLauncher.launch(intent) }
            .onFailure { viewModel.note("No file picker is available") }
    }

    fun placeInAr() {
        menuOpen = false
        viewModel.saveForAr {
            runCatching {
                context.startActivity(Intent(context, ArActivity::class.java))
            }.onFailure { viewModel.note("AR placement is not available") }
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PlumeColors.Ink),
    ) {
        PlumeCanvas(
            state = state,
            viewModel = viewModel,
            modifier = Modifier.fillMaxSize(),
        )

        if (menuOpen || stageMenuOpen) {
            Box(
                modifier = Modifier
                    .fillMaxSize()
                    .clickable(
                        interactionSource = dismissScrim,
                        indication = null,
                    ) {
                        menuOpen = false
                        stageMenuOpen = false
                    },
            )
        }

        FloatingTopBar(
            state = state,
            onMenuOpen = {
                stageMenuOpen = false
                menuOpen = !menuOpen
            },
            onHome = {
                menuOpen = false
                stageMenuOpen = false
                sheet = EditorSheet.Gallery
            },
            onUndo = viewModel::undo,
            onRedo = viewModel::redo,
            onFrame = viewModel::frameScene,
            onExport = {
                menuOpen = false
                stageMenuOpen = false
                sheet = EditorSheet.Export
            },
        )

        if (menuOpen) {
            ChromeMenuCard(
                modifier = Modifier
                    .align(Alignment.TopStart)
                    .statusBarsPadding()
                    .padding(start = 12.dp, top = 64.dp),
            ) {
                ChromeMenuRow("New blank scene") {
                    menuOpen = false
                    confirmNew = true
                }
                ChromeMenuRow("Scene gallery") {
                    menuOpen = false
                    sheet = EditorSheet.Gallery
                }
                ChromeMenuRow("Export scene") {
                    menuOpen = false
                    sheet = EditorSheet.Export
                }
                ChromeMenuRow("Place with AR") {
                    menuOpen = false
                    placeInAr()
                }
                ChromeMenuRow(
                    if (timelineVisible) "Hide timeline" else "Show timeline",
                ) {
                    timelineVisible = !timelineVisible
                    menuOpen = false
                }
            }
        }

        FloatingToolRail(
            state = state,
            onTool = viewModel::setTool,
            onBrushes = { sheet = EditorSheet.Brushes },
            modifier = Modifier
                .align(Alignment.CenterStart)
                .padding(start = 12.dp),
        )

        StageDock(
            timelineVisible = timelineVisible,
            menuOpen = stageMenuOpen,
            duration = state.animationDuration,
            onMenuOpen = {
                menuOpen = false
                stageMenuOpen = true
            },
            onScrub = viewModel::scrub,
            onStopPreview = viewModel::stopScrub,
            onNote = viewModel::note,
            onFrame = {
                stageMenuOpen = false
                viewModel.frameScene()
            },
            onOrbit = {
                stageMenuOpen = false
                viewModel.setTool(EditorTool.Orbit)
            },
            onAr = {
                stageMenuOpen = false
                placeInAr()
            },
            modifier = Modifier.align(Alignment.BottomCenter),
        )

        StatusCapsule(
            status = state.status,
            modifier = Modifier
                .align(Alignment.BottomStart)
                .navigationBarsPadding()
                .padding(start = 12.dp, bottom = 14.dp),
        )

        if (state.busy) {
            BusyOverlay(state.status)
        }
    }

    when (sheet) {
        EditorSheet.Gallery -> GallerySheet(
            state = state,
            onDismiss = { sheet = null },
            onLoad = { id ->
                viewModel.loadGallery(id)
                sheet = null
            },
        )

        EditorSheet.Brushes -> BrushSheet(
            state = state,
            onDismiss = { sheet = null },
            onSelect = { id ->
                viewModel.setBrush(id)
                sheet = null
            },
        )

        EditorSheet.Export -> ExportSheet(
            onDismiss = { sheet = null },
            onPick = { format -> startExport(format) },
        )

        null -> Unit
    }

    if (confirmNew) {
        AlertDialog(
            onDismissRequest = { confirmNew = false },
            containerColor = PlumeColors.Panel,
            titleContentColor = PlumeColors.Ivory,
            textContentColor = PlumeColors.Muted,
            title = { Text("Start a fresh sketch?") },
            text = {
                Text(
                    "The current scene stays in autosave until you draw or load another scene. " +
                        "This cannot be undone.",
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmNew = false
                        viewModel.newDocument()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = PlumeColors.Coral),
                ) {
                    Text("New scene", fontWeight = FontWeight.SemiBold)
                }
            },
            dismissButton = {
                TextButton(
                    onClick = { confirmNew = false },
                    colors = ButtonDefaults.textButtonColors(contentColor = PlumeColors.Muted),
                ) {
                    Text("Keep drawing")
                }
            },
        )
    }
}

@Composable
private fun FloatingTopBar(
    state: dev.plume.app.EditorState,
    onMenuOpen: () -> Unit,
    onHome: () -> Unit,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    onFrame: () -> Unit,
    onExport: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .statusBarsPadding()
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ChromeSurface {
            Row(
                modifier = Modifier.padding(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ChromeIconButton(
                    glyph = PlumeGlyph.Home,
                    label = "Scene gallery",
                    onClick = onHome,
                )
                ChromeRule()
                ChromeIconButton(
                    glyph = PlumeGlyph.Menu,
                    label = "Scene menu",
                    onClick = onMenuOpen,
                )
            }
        }

        Spacer(Modifier.weight(1f))

        ChromeSurface {
            Row(
                modifier = Modifier.padding(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                ChromeIconButton(
                    glyph = PlumeGlyph.Undo,
                    label = "Undo",
                    enabled = state.info?.canUndo == true,
                    onClick = onUndo,
                )
                ChromeIconButton(
                    glyph = PlumeGlyph.Redo,
                    label = "Redo",
                    enabled = state.info?.canRedo == true,
                    onClick = onRedo,
                )
                ChromeIconButton(
                    glyph = PlumeGlyph.Frame,
                    label = "Frame the artwork",
                    onClick = onFrame,
                )
                ChromeIconButton(
                    glyph = PlumeGlyph.Send,
                    label = "Export scene",
                    onClick = onExport,
                )
            }
        }
    }
}

@Composable
private fun FloatingToolRail(
    state: dev.plume.app.EditorState,
    onTool: (EditorTool) -> Unit,
    onBrushes: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(
        modifier = modifier,
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        ChromeSurface {
            Column(
                modifier = Modifier.padding(4.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                RailToolButton(
                    glyph = PlumeGlyph.Draw,
                    label = "Draw",
                    active = state.tool == EditorTool.Draw,
                    onClick = { onTool(EditorTool.Draw) },
                )
                RailToolButton(
                    glyph = PlumeGlyph.Erase,
                    label = "Erase",
                    active = state.tool == EditorTool.Erase,
                    onClick = { onTool(EditorTool.Erase) },
                )
                RailToolButton(
                    glyph = PlumeGlyph.Orbit,
                    label = "Orbit",
                    active = state.tool == EditorTool.Orbit,
                    onClick = { onTool(EditorTool.Orbit) },
                )
            }
        }

        ChromeSurface {
            Box(modifier = Modifier.padding(4.dp)) {
                BrushRailButton(state = state, onClick = onBrushes)
            }
        }
    }
}

@Composable
private fun BrushRailButton(
    state: dev.plume.app.EditorState,
    onClick: () -> Unit,
) {
    val brush = state.brushes.firstOrNull { it.id == state.selectedBrush }
    Box(
        modifier = Modifier
            .size(40.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(PlumeColors.InkSoft)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        BrushSwatch(
            modifier = Modifier.fillMaxSize().padding(horizontal = 7.dp, vertical = 8.dp),
            color = PlumeColors.CanvasText,
            weight = ((brush?.size ?: 5f) / 24f).coerceIn(0.12f, 1f),
        )
    }
}

@Composable
private fun StageDock(
    timelineVisible: Boolean,
    menuOpen: Boolean,
    duration: Double,
    onMenuOpen: () -> Unit,
    onScrub: (Double) -> Unit,
    onStopPreview: () -> Unit,
    onNote: (String) -> Unit,
    onFrame: () -> Unit,
    onOrbit: () -> Unit,
    onAr: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(
        modifier = modifier
            .fillMaxWidth()
            .navigationBarsPadding()
            .padding(bottom = 14.dp),
        contentAlignment = Alignment.BottomCenter,
    ) {
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            if (timelineVisible) {
                Surface(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(horizontal = 12.dp),
                    shape = RoundedCornerShape(18.dp),
                    color = PlumeColors.Chrome,
                    border = BorderStroke(1.dp, Color.White.copy(alpha = 0.55f)),
                    shadowElevation = 6.dp,
                ) {
                    TimelineBar(
                        duration = duration,
                        onScrub = onScrub,
                        onStopPreview = onStopPreview,
                        onNote = onNote,
                        modifier = Modifier.padding(horizontal = 14.dp, vertical = 8.dp),
                    )
                }
                Spacer(Modifier.height(10.dp))
            }

            if (menuOpen) {
                ChromeMenuCard(width = 190.dp) {
                    ChromeMenuRow("Frame artwork", onClick = onFrame)
                    ChromeMenuRow("Orbit the stage", onClick = onOrbit)
                    ChromeMenuRow("Place with AR", onClick = onAr)
                }
                Spacer(Modifier.height(10.dp))
            }

            Surface(
                onClick = onMenuOpen,
                shape = CircleShape,
                color = PlumeColors.Chrome,
                contentColor = PlumeColors.Ivory,
                border = BorderStroke(1.dp, Color.White.copy(alpha = 0.55f)),
                shadowElevation = 6.dp,
            ) {
                Row(
                    modifier = Modifier.padding(horizontal = 15.dp, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    PlumeIcon(
                        glyph = PlumeGlyph.Cube,
                        label = "Stage controls",
                        modifier = Modifier.size(16.dp),
                        tint = PlumeColors.Ivory,
                    )
                    Spacer(Modifier.width(6.dp))
                    Text(
                        text = "3D",
                        style = MaterialTheme.typography.labelSmall,
                        color = PlumeColors.Ivory,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }
        }
    }
}

@Composable
private fun ChromeMenuCard(
    modifier: Modifier = Modifier,
    width: Dp = 216.dp,
    content: @Composable () -> Unit,
) {
    Surface(
        modifier = modifier.width(width),
        shape = RoundedCornerShape(18.dp),
        color = PlumeColors.Panel,
        border = BorderStroke(1.dp, PlumeColors.LineSoft),
        shadowElevation = 12.dp,
    ) {
        Column(modifier = Modifier.padding(vertical = 6.dp)) {
            content()
        }
    }
}

@Composable
private fun ChromeMenuRow(
    label: String,
    onClick: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = FontWeight.Medium,
            color = PlumeColors.Ivory,
        )
    }
}

@Composable
private fun StatusCapsule(
    status: String,
    modifier: Modifier = Modifier,
) {
    Surface(
        modifier = modifier.widthIn(max = 186.dp),
        shape = RoundedCornerShape(16.dp),
        color = PlumeColors.Chrome,
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.55f)),
        shadowElevation = 4.dp,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(5.dp)
                    .clip(CircleShape)
                    .background(PlumeColors.Teal),
            )
            Spacer(Modifier.width(6.dp))
            Text(
                text = status,
                color = PlumeColors.Muted,
                style = MaterialTheme.typography.labelSmall.copy(letterSpacing = 0.2.sp),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Composable
private fun ChromeSurface(content: @Composable () -> Unit) {
    Surface(
        shape = RoundedCornerShape(18.dp),
        color = PlumeColors.Chrome,
        contentColor = PlumeColors.Ivory,
        border = BorderStroke(1.dp, Color.White.copy(alpha = 0.55f)),
        shadowElevation = 6.dp,
    ) {
        content()
    }
}

@Composable
private fun ChromeIconButton(
    glyph: PlumeGlyph,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(40.dp)
            .clip(CircleShape)
            .clickable(enabled = enabled, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        PlumeIcon(
            glyph = glyph,
            label = label,
            modifier = Modifier.size(20.dp),
            tint = if (enabled) PlumeColors.Ivory else PlumeColors.Faint.copy(alpha = 0.45f),
        )
    }
}

@Composable
private fun ChromeRule() {
    Box(
        modifier = Modifier
            .padding(horizontal = 2.dp)
            .width(1.dp)
            .height(20.dp)
            .background(PlumeColors.LineSoft),
    )
}

@Composable
private fun RailToolButton(
    glyph: PlumeGlyph,
    label: String,
    active: Boolean,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(40.dp)
            .clip(CircleShape)
            .background(if (active) PlumeColors.Ink.copy(alpha = 0.09f) else Color.Transparent)
            .clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        PlumeIcon(
            glyph = glyph,
            label = label,
            modifier = Modifier.size(20.dp),
            tint = if (active) PlumeColors.Coral else PlumeColors.Ivory,
        )
    }
}

@Composable
private fun BusyOverlay(status: String) {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(PlumeColors.Ink.copy(alpha = 0.45f)),
        contentAlignment = Alignment.Center,
    ) {
        Surface(
            color = PlumeColors.PanelHigh,
            shape = RoundedCornerShape(14.dp),
            border = BorderStroke(1.dp, PlumeColors.Line),
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                androidx.compose.material3.CircularProgressIndicator(
                    modifier = Modifier.size(18.dp),
                    color = PlumeColors.Coral,
                    strokeWidth = 2.dp,
                )
                Spacer(Modifier.width(10.dp))
                Text(
                    text = status,
                    style = MaterialTheme.typography.bodyMedium,
                    color = PlumeColors.Ivory,
                    maxLines = 2,
                )
            }
        }
    }
}

private fun suggestedName(documentName: String?, format: ExportFormat): String {
    val stem = documentName
        .orEmpty()
        .trim()
        .lowercase(Locale.US)
        .replace(Regex("[^a-z0-9]+"), "-")
        .trim('-')
        .ifBlank { "plume-sketch" }
    return "$stem.${format.extension}"
}
