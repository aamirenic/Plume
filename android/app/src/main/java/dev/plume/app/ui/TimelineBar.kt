package dev.plume.app.ui

import android.os.SystemClock
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay
import java.util.Locale
import kotlin.math.max

/**
 * Compact transport for camera shots and keyframes. Scrubbing paints the
 * animated camera into the software preview; returning restores the authoring
 * camera so drawing never happens from a stray shot.
 */
@Composable
fun TimelineBar(
    duration: Double,
    onScrub: (Double) -> Unit,
    onStopPreview: () -> Unit,
    onNote: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val total = duration.coerceAtLeast(0.1).toFloat()
    var time by remember { mutableFloatStateOf(0f) }
    var playing by remember { mutableStateOf(false) }
    var previewing by remember { mutableStateOf(false) }

    LaunchedEffect(playing, duration) {
        if (!playing) return@LaunchedEffect
        val from = if (time >= total - 0.03f) 0f else time
        val started = SystemClock.elapsedRealtime()
        previewing = true
        try {
            while (true) {
                val elapsed = (SystemClock.elapsedRealtime() - started) / 1000f
                val next = (from + elapsed).coerceAtMost(total)
                time = next
                onScrub(next.toDouble())
                if (next >= total - 0.001f) {
                    // Hold the final frame long enough to read it, then return
                    // to the authoring camera automatically.
                    delay(520)
                    break
                }
                delay(45)
            }
        } finally {
            playing = false
            previewing = false
            time = 0f
            onStopPreview()
        }
    }

    fun returnToCamera() {
        previewing = false
        time = 0f
        onStopPreview()
        onNote("Returned to your camera")
    }

    Column(modifier = modifier) {
        Row(
            modifier = Modifier.fillMaxWidth().height(22.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = "MOTION",
                color = PlumeColors.Gold,
                style = MaterialTheme.typography.labelSmall,
                letterSpacing = 1.6.sp,
            )
            Spacer(Modifier.width(8.dp))
            BoxLine()
            Spacer(Modifier.width(8.dp))
            Text(
                text = if (previewing) "CAMERA PREVIEW" else if (playing) "PLAYING" else "SHOTS + KEYS",
                color = if (previewing || playing) PlumeColors.Teal else PlumeColors.Faint,
                style = MaterialTheme.typography.labelSmall,
                maxLines = 1,
            )
            Spacer(Modifier.weight(1f))
            Text(
                text = formatTime(if (previewing || playing) time else 0f),
                color = PlumeColors.Muted,
                style = MaterialTheme.typography.labelSmall,
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Surface(
                onClick = { playing = !playing },
                enabled = duration > 0.1,
                shape = CircleShape,
                color = if (playing) PlumeColors.Teal else PlumeColors.PanelHigh,
                contentColor = if (playing) PlumeColors.Ink else PlumeColors.Ivory,
                modifier = Modifier.size(32.dp),
            ) {
                androidx.compose.foundation.layout.Box(
                    contentAlignment = Alignment.Center,
                ) {
                    PlumeIcon(
                        glyph = if (playing) PlumeGlyph.Pause else PlumeGlyph.Play,
                        label = if (playing) "Stop preview" else "Preview animation",
                        modifier = Modifier.size(14.dp),
                        tint = if (playing) PlumeColors.Ink else PlumeColors.Ivory,
                    )
                }
            }

            Slider(
                value = time.coerceIn(0f, total),
                onValueChange = { value ->
                    if (!playing) {
                        previewing = true
                        time = value
                        onScrub(value.toDouble())
                    }
                },
                onValueChangeFinished = {
                    if (!playing) {
                        previewing = true
                    }
                },
                valueRange = 0f..total,
                modifier = Modifier.weight(1f).height(30.dp),
                enabled = duration > 0.1,
                colors = SliderDefaults.colors(
                    thumbColor = if (previewing) PlumeColors.Gold else PlumeColors.Coral,
                    activeTrackColor = if (previewing) PlumeColors.Gold else PlumeColors.Coral,
                    inactiveTrackColor = PlumeColors.Line,
                    disabledThumbColor = PlumeColors.Faint,
                    disabledActiveTrackColor = PlumeColors.Line,
                    disabledInactiveTrackColor = PlumeColors.Line.copy(alpha = 0.6f),
                ),
            )

            Text(
                text = formatTime(total),
                color = PlumeColors.Faint,
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.width(38.dp),
            )

            Surface(
                onClick = { if (previewing) returnToCamera() },
                enabled = previewing,
                shape = CircleShape,
                color = Color.Transparent,
                contentColor = if (previewing) PlumeColors.Gold else PlumeColors.Faint,
                modifier = Modifier.size(30.dp),
            ) {
                androidx.compose.foundation.layout.Box(
                    modifier = Modifier.clip(CircleShape).background(
                        if (previewing) PlumeColors.Gold.copy(alpha = 0.10f) else Color.Transparent,
                    ),
                    contentAlignment = Alignment.Center,
                ) {
                    PlumeIcon(
                        glyph = PlumeGlyph.Return,
                        label = "Return to authoring camera",
                        modifier = Modifier.size(16.dp),
                        tint = if (previewing) PlumeColors.Gold else PlumeColors.Faint.copy(alpha = 0.45f),
                    )
                }
            }
        }
    }
}

@Composable
private fun BoxLine() {
    androidx.compose.foundation.layout.Box(
        modifier = Modifier
            .width(18.dp)
            .height(1.dp)
            .background(PlumeColors.Line),
    )
}

private fun formatTime(seconds: Float): String {
    val clamped = max(seconds, 0f)
    return String.format(Locale.US, "%.1fs", clamped)
}
