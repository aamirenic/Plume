package dev.plume.app.ui

import android.app.Activity
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
        Column(
            modifier = Modifier
                .fillMaxSize()
                .statusBarsPadding(),
        ) {
            EditorTopBar(
                state = state,
                onUndo = viewModel::undo,
                onRedo = viewModel::redo,
                menuOpen = menuOpen,
                onMenuOpen = { menuOpen = true },
                onMenuDismiss = { menuOpen = false },
                onNew = {
                    menuOpen = false
                    confirmNew = true
                },
                onGallery = {
                    menuOpen = false
                    sheet = EditorSheet.Gallery
                },
                onExport = {
                    menuOpen = false
                    sheet = EditorSheet.Export
                },
                onAr = { placeInAr() },
            )

            Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
                PlumeCanvas(
                    state = state,
                    viewModel = viewModel,
                    modifier = Modifier.fillMaxSize(),
                )

                CanvasChrome(
                    state = state,
                    onFrame = viewModel::frameScene,
                    onAr = { placeInAr() },
                    onBrushes = { sheet = EditorSheet.Brushes },
                )
            }

            TimelineBar(
                duration = state.animationDuration,
                onScrub = viewModel::scrub,
                onStopPreview = viewModel::stopScrub,
                onNote = viewModel::note,
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 12.dp, vertical = 4.dp),
            )

            BottomDock(
                state = state,
                onTool = viewModel::setTool,
                onBrushes = { sheet = EditorSheet.Brushes },
                onGallery = { sheet = EditorSheet.Gallery },
                onExport = { sheet = EditorSheet.Export },
                modifier = Modifier
                    .fillMaxWidth()
                    .navigationBarsPadding(),
            )
        }

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
private fun EditorTopBar(
    state: dev.plume.app.EditorState,
    onUndo: () -> Unit,
    onRedo: () -> Unit,
    menuOpen: Boolean,
    onMenuOpen: () -> Unit,
    onMenuDismiss: () -> Unit,
    onNew: () -> Unit,
    onGallery: () -> Unit,
    onExport: () -> Unit,
    onAr: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .height(58.dp)
            .padding(horizontal = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        PlumeMark(modifier = Modifier.size(26.dp))
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(
                text = "PLUME",
                color = PlumeColors.Ivory,
                style = MaterialTheme.typography.titleMedium,
                fontWeight = FontWeight.SemiBold,
                letterSpacing = 4.5.sp,
            )
            Text(
                text = state.info?.name ?: "Opening studio",
                color = PlumeColors.Muted,
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }

        TopBarButton(
            glyph = PlumeGlyph.Undo,
            label = "Undo",
            enabled = state.info?.canUndo == true,
            onClick = onUndo,
        )
        TopBarButton(
            glyph = PlumeGlyph.Redo,
            label = "Redo",
            enabled = state.info?.canRedo == true,
            onClick = onRedo,
        )

        Box {
            TopBarButton(
                glyph = PlumeGlyph.Menu,
                label = "Scene menu",
                onClick = onMenuOpen,
            )
            DropdownMenu(
                expanded = menuOpen,
                onDismissRequest = onMenuDismiss,
            ) {
                DropdownMenuItem(
                    text = { Text("Scene gallery") },
                    onClick = onGallery,
                )
                DropdownMenuItem(
                    text = { Text("New blank scene") },
                    onClick = onNew,
                )
                DropdownMenuItem(
                    text = { Text("Send it out") },
                    onClick = onExport,
                )
                DropdownMenuItem(
                    text = { Text("Place with AR") },
                    onClick = onAr,
                )
                DropdownMenuItem(
                    text = { Text(if (state.autosavedAt > 0) "Saved on this device" else "Not saved yet") },
                    enabled = false,
                    onClick = {},
                )
            }
        }
    }
    Hairline()
}

@Composable
private fun TopBarButton(
    glyph: PlumeGlyph,
    label: String,
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    Box(
        modifier = Modifier
            .size(40.dp)
            .clip(RoundedCornerShape(10.dp))
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
private fun CanvasChrome(
    state: dev.plume.app.EditorState,
    onFrame: () -> Unit,
    onAr: () -> Unit,
    onBrushes: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BrushChip(
            state = state,
            onClick = onBrushes,
            modifier = Modifier.widthIn(max = 150.dp),
        )
        Spacer(Modifier.weight(1f))
        CirclePlate(glyph = PlumeGlyph.Frame, label = "Frame the scene", onClick = onFrame)
        Spacer(Modifier.width(8.dp))
        Surface(
            onClick = onAr,
            color = PlumeColors.Panel.copy(alpha = 0.88f),
            contentColor = PlumeColors.Teal,
            shape = CircleShape,
            border = androidx.compose.foundation.BorderStroke(1.dp, PlumeColors.Line),
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                PlumeIcon(PlumeGlyph.Ar, "Place with AR", Modifier.size(16.dp), PlumeColors.Teal)
                Spacer(Modifier.width(5.dp))
                Text(
                    text = "AR",
                    style = MaterialTheme.typography.labelSmall,
                    color = PlumeColors.Teal,
                    fontWeight = FontWeight.SemiBold,
                )
            }
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 12.dp)
            .padding(bottom = 14.dp),
    ) {
        StatusPill(
            status = state.status,
            modifier = Modifier.align(Alignment.BottomStart),
        )
    }
}

@Composable
private fun CirclePlate(
    glyph: PlumeGlyph,
    label: String,
    onClick: () -> Unit,
) {
    Surface(
        onClick = onClick,
        shape = CircleShape,
        color = PlumeColors.Panel.copy(alpha = 0.88f),
        contentColor = PlumeColors.Ivory,
        border = androidx.compose.foundation.BorderStroke(1.dp, PlumeColors.Line),
        modifier = Modifier.size(40.dp),
    ) {
        Box(contentAlignment = Alignment.Center) {
            PlumeIcon(glyph, label, Modifier.size(18.dp), PlumeColors.Ivory)
        }
    }
}

@Composable
private fun StatusPill(status: String, modifier: Modifier = Modifier) {
    Row(
        modifier = modifier
            .widthIn(max = 310.dp)
            .clip(RoundedCornerShape(18.dp))
            .background(PlumeColors.Ink.copy(alpha = 0.78f))
            .border(1.dp, PlumeColors.Line.copy(alpha = 0.8f), RoundedCornerShape(18.dp))
            .padding(horizontal = 11.dp, vertical = 7.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(5.dp)
                .clip(CircleShape)
                .background(PlumeColors.Teal),
        )
        Spacer(Modifier.width(7.dp))
        Text(
            text = status,
            color = PlumeColors.Muted,
            style = MaterialTheme.typography.labelSmall,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun BrushChip(
    state: dev.plume.app.EditorState,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val brush = state.brushes.firstOrNull { it.id == state.selectedBrush }
    Row(
        modifier = modifier
            .clip(RoundedCornerShape(18.dp))
            .background(PlumeColors.Panel.copy(alpha = 0.9f))
            .border(1.dp, PlumeColors.Line, RoundedCornerShape(18.dp))
            .clickable(onClick = onClick)
            .padding(start = 7.dp, end = 11.dp, top = 5.dp, bottom = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(width = 38.dp, height = 22.dp)
                .clip(RoundedCornerShape(6.dp))
                .background(PlumeColors.InkSoft),
            contentAlignment = Alignment.Center,
        ) {
            BrushSwatch(
                modifier = Modifier.fillMaxSize().padding(horizontal = 5.dp),
                color = PlumeColors.Coral,
                weight = ((brush?.size ?: 5f) / 24f).coerceIn(0.12f, 1f),
            )
        }
        Spacer(Modifier.width(7.dp))
        Text(
            text = brush?.name ?: "Choose brush",
            color = PlumeColors.Ivory,
            style = MaterialTheme.typography.labelSmall,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun BottomDock(
    state: dev.plume.app.EditorState,
    onTool: (EditorTool) -> Unit,
    onBrushes: () -> Unit,
    onGallery: () -> Unit,
    onExport: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.background(PlumeColors.Panel)) {
        Hairline()
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(66.dp)
                .padding(horizontal = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            DockItem(
                glyph = PlumeGlyph.Draw,
                label = "Draw",
                active = state.tool == EditorTool.Draw,
                onClick = { onTool(EditorTool.Draw) },
                modifier = Modifier.weight(1f),
            )
            DockItem(
                glyph = PlumeGlyph.Erase,
                label = "Erase",
                active = state.tool == EditorTool.Erase,
                onClick = { onTool(EditorTool.Erase) },
                modifier = Modifier.weight(1f),
            )
            DockItem(
                glyph = PlumeGlyph.Orbit,
                label = "Orbit",
                active = state.tool == EditorTool.Orbit,
                onClick = { onTool(EditorTool.Orbit) },
                modifier = Modifier.weight(1f),
            )
            DockItem(
                glyph = PlumeGlyph.Brushes,
                label = "Brushes",
                active = false,
                onClick = onBrushes,
                modifier = Modifier.weight(1f),
            )
            DockItem(
                glyph = PlumeGlyph.Scenes,
                label = "Scenes",
                active = false,
                onClick = onGallery,
                modifier = Modifier.weight(1f),
            )
            DockItem(
                glyph = PlumeGlyph.Send,
                label = "Send",
                active = false,
                onClick = onExport,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

@Composable
private fun DockItem(
    glyph: PlumeGlyph,
    label: String,
    active: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val tint = if (active) PlumeColors.Coral else PlumeColors.Muted
    Column(
        modifier = modifier
            .height(56.dp)
            .clip(RoundedCornerShape(10.dp))
            .background(if (active) PlumeColors.Coral.copy(alpha = 0.10f) else Color.Transparent)
            .clickable(onClick = onClick),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        PlumeIcon(glyph, label, Modifier.size(21.dp), tint)
        Spacer(Modifier.height(3.dp))
        Text(
            text = label,
            color = tint,
            style = MaterialTheme.typography.labelSmall,
            fontWeight = if (active) FontWeight.SemiBold else FontWeight.Medium,
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
            border = androidx.compose.foundation.BorderStroke(1.dp, PlumeColors.Line),
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

@Composable
private fun Hairline() {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(1.dp)
            .background(PlumeColors.Line.copy(alpha = 0.7f)),
    )
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
