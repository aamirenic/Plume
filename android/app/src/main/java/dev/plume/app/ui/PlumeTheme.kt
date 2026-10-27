package dev.plume.app.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/**
 * PLUME's palette is original: warm ivory ink on a charcoal studio, coral for
 * the active tool, teal for secondary state, and muted gold for time.
 */
object PlumeColors {
    val Ink = Color(0xFF0D0F12)
    val InkSoft = Color(0xFF12151A)
    val Panel = Color(0xFF171A1F)
    val PanelHigh = Color(0xFF1E2229)
    val Ivory = Color(0xFFF2EEE4)
    val Muted = Color(0xFFA7A9A5)
    val Faint = Color(0xFF6E726F)
    val Coral = Color(0xFFF27362)
    val Teal = Color(0xFF59BFC4)
    val Gold = Color(0xFFD9B56B)
    val Line = Color(0xFF2A2E34)
    val Danger = Color(0xFFE05B4D)
}

private val PlumeScheme = darkColorScheme(
    primary = PlumeColors.Coral,
    onPrimary = PlumeColors.Ink,
    primaryContainer = Color(0xFF48231E),
    onPrimaryContainer = PlumeColors.Ivory,
    secondary = PlumeColors.Teal,
    onSecondary = PlumeColors.Ink,
    secondaryContainer = Color(0xFF173A3D),
    onSecondaryContainer = PlumeColors.Ivory,
    tertiary = PlumeColors.Gold,
    onTertiary = PlumeColors.Ink,
    background = PlumeColors.Ink,
    onBackground = PlumeColors.Ivory,
    surface = PlumeColors.Panel,
    onSurface = PlumeColors.Ivory,
    surfaceVariant = PlumeColors.PanelHigh,
    onSurfaceVariant = PlumeColors.Muted,
    outline = PlumeColors.Line,
    outlineVariant = Color(0xFF20242A),
    error = PlumeColors.Danger,
)

private val PlumeTypography = Typography(
    displayLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Light,
        fontSize = 46.sp,
        letterSpacing = 8.sp,
    ),
    headlineMedium = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Medium,
        fontSize = 22.sp,
        letterSpacing = 1.2.sp,
    ),
    titleLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.SemiBold,
        fontSize = 18.sp,
        letterSpacing = 0.4.sp,
    ),
    titleMedium = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
        letterSpacing = 0.3.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Normal,
        fontSize = 13.sp,
        letterSpacing = 0.15.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.SemiBold,
        fontSize = 12.sp,
        letterSpacing = 0.8.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = FontFamily.SansSerif,
        fontWeight = FontWeight.Medium,
        fontSize = 10.sp,
        letterSpacing = 1.1.sp,
    ),
)

private val PlumeShapes = Shapes(
    extraSmall = androidx.compose.foundation.shape.RoundedCornerShape(4.dp),
    small = androidx.compose.foundation.shape.RoundedCornerShape(8.dp),
    medium = androidx.compose.foundation.shape.RoundedCornerShape(12.dp),
    large = androidx.compose.foundation.shape.RoundedCornerShape(18.dp),
    extraLarge = androidx.compose.foundation.shape.RoundedCornerShape(26.dp),
)

@Composable
fun PlumeTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = PlumeScheme,
        typography = PlumeTypography,
        shapes = PlumeShapes,
        content = content,
    )
}

/** Original three-ribbon mark, drawn procedurally so no image assets ship. */
@Composable
fun PlumeMark(modifier: Modifier = Modifier, color: Color = PlumeColors.Ivory) {
    Canvas(modifier = modifier.size(26.dp)) {
        val w = size.width
        val h = size.height
        val coral = Path().apply {
            moveTo(w * 0.16f, h * 0.82f)
            cubicTo(w * 0.26f, h * 0.46f, w * 0.48f, h * 0.20f, w * 0.80f, h * 0.16f)
            cubicTo(w * 0.60f, h * 0.38f, w * 0.48f, h * 0.58f, w * 0.40f, h * 0.84f)
            cubicTo(w * 0.33f, h * 0.78f, w * 0.24f, h * 0.76f, w * 0.16f, h * 0.82f)
            close()
        }
        drawPath(coral, PlumeColors.Coral)
        val ivory = Path().apply {
            moveTo(w * 0.42f, h * 0.86f)
            cubicTo(w * 0.50f, h * 0.58f, w * 0.66f, h * 0.38f, w * 0.90f, h * 0.32f)
            cubicTo(w * 0.76f, h * 0.48f, w * 0.66f, h * 0.64f, w * 0.60f, h * 0.88f)
            cubicTo(w * 0.54f, h * 0.82f, w * 0.48f, h * 0.82f, w * 0.42f, h * 0.86f)
            close()
        }
        drawPath(ivory, color)
        val teal = Path().apply {
            moveTo(w * 0.60f, h * 0.90f)
            cubicTo(w * 0.66f, h * 0.72f, w * 0.76f, h * 0.60f, w * 0.94f, h * 0.55f)
            cubicTo(w * 0.84f, h * 0.66f, w * 0.78f, h * 0.76f, w * 0.74f, h * 0.92f)
            cubicTo(w * 0.69f, h * 0.87f, w * 0.65f, h * 0.87f, w * 0.60f, h * 0.90f)
            close()
        }
        drawPath(teal, PlumeColors.Teal)
    }
}

/** A thin underline accent used under section headings. */
@Composable
fun RibbonDivider(modifier: Modifier = Modifier) {
    Canvas(modifier = modifier) {
        val y = size.height * 0.5f
        drawLine(
            color = PlumeColors.Coral,
            start = Offset(0f, y),
            end = Offset(size.width * 0.62f, y),
            strokeWidth = 1.4f,
        )
        drawLine(
            color = PlumeColors.Teal,
            start = Offset(size.width * 0.66f, y),
            end = Offset(size.width, y),
            strokeWidth = 1.4f,
        )
    }
}

/** Procedural brush swatch: a tapered stroke whose weight follows the brush. */
@Composable
fun BrushSwatch(
    modifier: Modifier = Modifier,
    color: Color = PlumeColors.Ivory,
    weight: Float = 0.5f,
    dotted: Boolean = false,
    flat: Boolean = false,
) {
    Canvas(modifier = modifier) {
        val h = size.height
        val w = size.width
        val path = Path().apply {
            moveTo(w * 0.06f, h * 0.62f)
            cubicTo(w * 0.28f, h * 0.18f, w * 0.62f, h * 0.86f, w * 0.94f, h * 0.34f)
        }
        val stroke = (1.0f + weight * 4.0f).dp.toPx()
        drawPath(
            path = path,
            color = color,
            style = Stroke(width = if (flat) stroke * 1.6f else stroke),
        )
        if (dotted) {
            for (i in 0..4) {
                val x = w * (0.18f + i * 0.16f)
                val y = h * (0.52f + (i % 2) * 0.10f)
                drawCircle(color, radius = (1.2f + weight * 1.4f).dp.toPx(), center = Offset(x, y))
            }
        }
    }
}
