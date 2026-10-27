package dev.plume.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/**
 * The icon set is drawn from primitives so the app ships without borrowed
 * artwork.  Glyphs stay geometric and a little calligraphic: round caps, open
 * curves, and no filled UI tropes beyond the transport controls.
 */
enum class PlumeGlyph {
    Undo,
    Redo,
    Menu,
    Close,
    Play,
    Pause,
    Return,
    Draw,
    Erase,
    Orbit,
    Brushes,
    Scenes,
    Send,
    Frame,
    Ar,
    Cube,
}

@Composable
fun PlumeIcon(
    glyph: PlumeGlyph,
    label: String,
    modifier: Modifier = Modifier,
    tint: Color = PlumeColors.Ivory,
) {
    Canvas(
        modifier = modifier.semantics { contentDescription = label },
    ) {
        val w = size.width
        val h = size.height
        val unit = minOf(w, h)
        val stroke = (unit * 0.085f).coerceAtLeast(1f)
        val line = Stroke(width = stroke, cap = StrokeCap.Round)
        val thin = Stroke(width = (stroke * 0.72f).coerceAtLeast(1f), cap = StrokeCap.Round)

        fun path(build: Path.() -> Unit): Path = Path().apply(build)

        when (glyph) {
            PlumeGlyph.Undo, PlumeGlyph.Redo -> {
                val flip = glyph == PlumeGlyph.Redo
                withTransform({
                    if (flip) {
                        scale(scaleX = -1f, scaleY = 1f, pivot = Offset(w / 2f, h / 2f))
                    }
                }) {
                    val p = path {
                        moveTo(w * 0.79f, h * 0.33f)
                        cubicTo(w * 0.58f, h * 0.13f, w * 0.22f, h * 0.25f, w * 0.22f, h * 0.55f)
                        cubicTo(w * 0.22f, h * 0.74f, w * 0.38f, h * 0.84f, w * 0.55f, h * 0.83f)
                    }
                    drawPath(p, tint, style = line)
                    drawLine(tint, Offset(w * 0.22f, h * 0.55f), Offset(w * 0.10f, h * 0.44f), stroke, StrokeCap.Round)
                    drawLine(tint, Offset(w * 0.22f, h * 0.55f), Offset(w * 0.34f, h * 0.44f), stroke, StrokeCap.Round)
                }
            }

            PlumeGlyph.Menu -> {
                drawLine(tint, Offset(w * 0.20f, h * 0.34f), Offset(w * 0.80f, h * 0.34f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.20f, h * 0.52f), Offset(w * 0.64f, h * 0.52f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.20f, h * 0.70f), Offset(w * 0.72f, h * 0.70f), stroke, StrokeCap.Round)
            }

            PlumeGlyph.Close -> {
                drawLine(tint, Offset(w * 0.30f, h * 0.30f), Offset(w * 0.70f, h * 0.70f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.70f, h * 0.30f), Offset(w * 0.30f, h * 0.70f), stroke, StrokeCap.Round)
            }

            PlumeGlyph.Play -> {
                val p = path {
                    moveTo(w * 0.36f, h * 0.24f)
                    lineTo(w * 0.76f, h * 0.50f)
                    lineTo(w * 0.36f, h * 0.76f)
                    close()
                }
                drawPath(p, tint)
            }

            PlumeGlyph.Pause -> {
                drawRoundRect(
                    tint,
                    topLeft = Offset(w * 0.31f, h * 0.26f),
                    size = androidx.compose.ui.geometry.Size(w * 0.13f, h * 0.48f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(stroke * 0.6f),
                )
                drawRoundRect(
                    tint,
                    topLeft = Offset(w * 0.56f, h * 0.26f),
                    size = androidx.compose.ui.geometry.Size(w * 0.13f, h * 0.48f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(stroke * 0.6f),
                )
            }

            PlumeGlyph.Return -> {
                drawArc(
                    color = tint,
                    startAngle = 35f,
                    sweepAngle = 285f,
                    useCenter = false,
                    topLeft = Offset(w * 0.21f, h * 0.21f),
                    size = androidx.compose.ui.geometry.Size(w * 0.58f, h * 0.58f),
                    style = line,
                )
                drawLine(tint, Offset(w * 0.21f, h * 0.42f), Offset(w * 0.21f, h * 0.65f), stroke, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.21f, h * 0.65f), Offset(w * 0.43f, h * 0.65f), stroke, StrokeCap.Round)
            }

            PlumeGlyph.Draw -> {
                val nib = path {
                    moveTo(w * 0.24f, h * 0.76f)
                    lineTo(w * 0.32f, h * 0.44f)
                    lineTo(w * 0.64f, h * 0.22f)
                    lineTo(w * 0.78f, h * 0.36f)
                    lineTo(w * 0.52f, h * 0.66f)
                    close()
                }
                drawPath(nib, tint, style = thin)
                drawCircle(tint, radius = stroke * 0.72f, center = Offset(w * 0.36f, h * 0.68f))
                drawLine(tint, Offset(w * 0.26f, h * 0.74f), Offset(w * 0.18f, h * 0.84f), stroke, StrokeCap.Round)
            }

            PlumeGlyph.Erase -> {
                withTransform({ rotate(degrees = -38f, pivot = Offset(w * 0.5f, h * 0.5f)) }) {
                    drawRoundRect(
                        color = tint,
                        topLeft = Offset(w * 0.24f, h * 0.36f),
                        size = androidx.compose.ui.geometry.Size(w * 0.50f, h * 0.28f),
                        cornerRadius = androidx.compose.ui.geometry.CornerRadius(stroke * 0.55f),
                        style = thin,
                    )
                    drawLine(tint, Offset(w * 0.47f, h * 0.37f), Offset(w * 0.47f, h * 0.63f), thin.width, StrokeCap.Round)
                }
                drawLine(tint, Offset(w * 0.22f, h * 0.81f), Offset(w * 0.62f, h * 0.81f), thin.width, StrokeCap.Round)
            }

            PlumeGlyph.Orbit -> {
                drawOval(
                    color = tint,
                    topLeft = Offset(w * 0.16f, h * 0.30f),
                    size = androidx.compose.ui.geometry.Size(w * 0.68f, h * 0.40f),
                    style = thin,
                )
                drawCircle(tint, radius = stroke * 0.95f, center = Offset(w * 0.50f, h * 0.50f))
                drawCircle(tint, radius = stroke * 0.72f, center = Offset(w * 0.82f, h * 0.36f))
            }

            PlumeGlyph.Brushes -> {
                val p = path {
                    moveTo(w * 0.22f, h * 0.70f)
                    cubicTo(w * 0.34f, h * 0.24f, w * 0.58f, h * 0.18f, w * 0.74f, h * 0.30f)
                    cubicTo(w * 0.80f, h * 0.34f, w * 0.74f, h * 0.44f, w * 0.64f, h * 0.42f)
                    cubicTo(w * 0.48f, h * 0.40f, w * 0.42f, h * 0.60f, w * 0.44f, h * 0.78f)
                }
                drawPath(p, tint, style = line)
                drawCircle(tint, radius = stroke * 0.85f, center = Offset(w * 0.28f, h * 0.74f))
            }

            PlumeGlyph.Scenes -> {
                val diamond = path {
                    moveTo(w * 0.50f, h * 0.20f)
                    lineTo(w * 0.79f, h * 0.38f)
                    lineTo(w * 0.50f, h * 0.56f)
                    lineTo(w * 0.21f, h * 0.38f)
                    close()
                }
                drawPath(diamond, tint, style = thin)
                drawLine(tint, Offset(w * 0.21f, h * 0.52f), Offset(w * 0.50f, h * 0.70f), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.50f, h * 0.70f), Offset(w * 0.79f, h * 0.52f), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.21f, h * 0.64f), Offset(w * 0.50f, h * 0.82f), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * 0.50f, h * 0.82f), Offset(w * 0.79f, h * 0.64f), thin.width, StrokeCap.Round)
            }

            PlumeGlyph.Send -> {
                val plane = path {
                    moveTo(w * 0.18f, h * 0.50f)
                    lineTo(w * 0.82f, h * 0.22f)
                    lineTo(w * 0.60f, h * 0.80f)
                    lineTo(w * 0.48f, h * 0.58f)
                    close()
                }
                drawPath(plane, tint, style = thin)
                drawLine(tint, Offset(w * 0.48f, h * 0.58f), Offset(w * 0.82f, h * 0.22f), thin.width, StrokeCap.Round)
            }

            PlumeGlyph.Frame -> {
                val inset = 0.25f
                val arm = 0.17f
                drawLine(tint, Offset(w * inset, h * (inset + arm)), Offset(w * inset, h * inset), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * inset, h * inset), Offset(w * (inset + arm), h * inset), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * (1 - inset - arm), h * inset), Offset(w * (1 - inset), h * inset), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * (1 - inset), h * inset), Offset(w * (1 - inset), h * (inset + arm)), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * (1 - inset), h * (1 - inset - arm)), Offset(w * (1 - inset), h * (1 - inset)), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * (1 - inset), h * (1 - inset)), Offset(w * (1 - inset - arm), h * (1 - inset)), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * (inset + arm), h * (1 - inset)), Offset(w * inset, h * (1 - inset)), thin.width, StrokeCap.Round)
                drawLine(tint, Offset(w * inset, h * (1 - inset)), Offset(w * inset, h * (1 - inset - arm)), thin.width, StrokeCap.Round)
            }

            PlumeGlyph.Ar, PlumeGlyph.Cube -> {
                val top = Offset(w * 0.50f, h * 0.18f)
                val left = Offset(w * 0.20f, h * 0.35f)
                val right = Offset(w * 0.80f, h * 0.35f)
                val bottom = Offset(w * 0.50f, h * 0.52f)
                val leftLow = Offset(w * 0.20f, h * 0.68f)
                val rightLow = Offset(w * 0.80f, h * 0.68f)
                val bottomLow = Offset(w * 0.50f, h * 0.85f)
                for ((a, b) in listOf(
                    top to left,
                    top to right,
                    left to bottom,
                    right to bottom,
                    left to leftLow,
                    right to rightLow,
                    bottom to bottomLow,
                    leftLow to bottomLow,
                    rightLow to bottomLow,
                )) {
                    drawLine(tint, a, b, thin.width, StrokeCap.Round)
                }
                if (glyph == PlumeGlyph.Ar) {
                    drawCircle(tint, radius = stroke * 0.58f, center = Offset(w * 0.50f, h * 0.52f))
                    drawArc(
                        color = tint,
                        startAngle = 210f,
                        sweepAngle = 120f,
                        useCenter = false,
                        topLeft = Offset(w * 0.34f, h * 0.42f),
                        size = androidx.compose.ui.geometry.Size(w * 0.32f, h * 0.24f),
                        style = Stroke(width = thin.width * 0.8f, cap = StrokeCap.Round),
                    )
                }
            }
        }
    }
}

/** Small rounded-square hit target used by the editor chrome. */
@Composable
fun GlyphPlate(
    glyph: PlumeGlyph,
    label: String,
    active: Boolean = false,
    enabled: Boolean = true,
    tint: Color = if (active) PlumeColors.Coral else PlumeColors.Ivory,
    modifier: Modifier = Modifier,
) {
    val bg = if (active) PlumeColors.Coral.copy(alpha = 0.12f) else Color.Transparent
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(10.dp))
            .background(bg),
        contentAlignment = Alignment.Center,
    ) {
        PlumeIcon(
            glyph = glyph,
            label = label,
            modifier = Modifier.size(20.dp),
            tint = if (enabled) tint else PlumeColors.Faint.copy(alpha = 0.55f),
        )
    }
}
