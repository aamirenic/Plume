package dev.plume.app.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.unit.dp
import dev.plume.app.EditorState
import dev.plume.app.PlumeViewModel
import kotlin.math.abs
import kotlin.math.max

private const val TAP_SLOP = 16f

@Composable
fun PlumeCanvas(
    state: EditorState,
    viewModel: PlumeViewModel,
    modifier: Modifier = Modifier,
) {
    val tool by rememberUpdatedState(state.tool)
    val hasInk = (state.info?.curves?.toInt() ?: 0) > 0

    Box(
        modifier = modifier
            .background(PlumeColors.Ink)
            .onSizeChanged { viewModel.updateViewport(it.width, it.height) },
    ) {
        val bitmap = state.preview
        if (bitmap != null) {
            Image(
                bitmap = bitmap.asImageBitmap(),
                contentDescription = "PLUME sketch preview",
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Fit,
            )
        } else {
            CircularProgressIndicator(
                modifier = Modifier.align(Alignment.Center),
                color = PlumeColors.Coral,
                strokeWidth = 2.dp,
            )
        }

        if (!hasInk) {
            Column(
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(32.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = "The stage is empty",
                    style = MaterialTheme.typography.titleMedium,
                    color = PlumeColors.Ivory,
                )
                Text(
                    text = "Drag a stroke, or open the gallery for a starting scene.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = PlumeColors.Muted,
                )
            }
        }

        // Transparent input surface. Keeping it above the bitmap also means the
        // canvas receives events over the letterboxed preview edges.
        Box(
            modifier = Modifier
                .fillMaxSize()
                .clip(RoundedCornerShape(0.dp))
                .pointerInput(tool) {
                    awaitEachGesture {
                        val first = awaitFirstDown(requireUnconsumed = false)
                        var drawing = tool == EditorTool.Draw
                        var navigating = tool == EditorTool.Orbit
                        var erasing = tool == EditorTool.Erase
                        var lastSingle = first.position
                        var travel = 0f
                        var previousCentroid: Offset? = null
                        var previousSpan = 0f

                        if (drawing) {
                            viewModel.beginStroke(
                                first.position.x,
                                first.position.y,
                                first.pressure(),
                            )
                        }

                        var gestureActive = true
                        while (gestureActive) {
                            val event = awaitPointerEvent()
                            val pressed = event.changes.filter { it.pressed }
                            if (pressed.isEmpty()) {
                                gestureActive = false
                                break
                            }

                            if (pressed.size >= 2) {
                                if (drawing) {
                                    // A second finger means navigation, not a
                                    // second stroke. Drop the partial mark.
                                    viewModel.cancelStroke()
                                    drawing = false
                                }
                                erasing = false
                                navigating = true
                                val points = pressed.take(3)
                                val centroid = points.fold(Offset.Zero) { acc, change ->
                                    acc + change.position
                                } / points.size.toFloat()
                                val span = (points[0].position - points[1].position)
                                    .getDistance()
                                    .coerceAtLeast(1f)
                                val lastCentroid = previousCentroid
                                val lastSpan = previousSpan
                                if (lastCentroid != null && lastSpan > 0f) {
                                    val dx = centroid.x - lastCentroid.x
                                    val dy = centroid.y - lastCentroid.y
                                    if (points.size >= 3) {
                                        viewModel.pan(dx, dy)
                                    } else if (max(abs(dx), abs(dy)) > 0.4f) {
                                        viewModel.orbit(dx, dy)
                                    }
                                    if (span > 1f && abs(span - lastSpan) > 0.5f) {
                                        viewModel.zoom((span / lastSpan).coerceIn(0.82f, 1.22f))
                                    }
                                }
                                previousCentroid = centroid
                                previousSpan = span
                                pressed.forEach { it.consume() }
                            } else {
                                val change = pressed.first()
                                if (drawing) {
                                    travel += (change.position - lastSingle).getDistance()
                                    viewModel.extendStroke(
                                        change.position.x,
                                        change.position.y,
                                        change.pressure(),
                                    )
                                    lastSingle = change.position
                                    change.consume()
                                } else if (erasing) {
                                    travel += (change.position - lastSingle).getDistance()
                                    lastSingle = change.position
                                    change.consume()
                                } else if (tool == EditorTool.Orbit || navigating) {
                                    if (tool == EditorTool.Orbit) {
                                        val delta = change.position - lastSingle
                                        viewModel.orbit(delta.x, delta.y)
                                        lastSingle = change.position
                                    }
                                    change.consume()
                                }
                                previousCentroid = null
                                previousSpan = 0f
                            }
                        }

                        when {
                            drawing -> viewModel.endStroke()
                            erasing && travel < TAP_SLOP ->
                                viewModel.eraseAt(first.position.x, first.position.y)
                            navigating -> viewModel.finishCameraGesture()
                        }
                    }
                },
        )
    }
}

private fun PointerInputChange.pressure(): Float {
    val value = pressure
    return if (value.isFinite() && value > 0.01f) value.coerceAtMost(1f) else 0.65f
}
