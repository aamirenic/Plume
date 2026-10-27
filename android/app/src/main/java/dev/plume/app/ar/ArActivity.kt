package dev.plume.app.ar

import dev.plume.app.R
import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.PackageManager
import android.graphics.Color
import android.opengl.GLSurfaceView
import android.os.Build
import android.os.Bundle
import android.view.MotionEvent
import android.view.WindowManager
import android.view.GestureDetector
import android.view.ScaleGestureDetector
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
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
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color as ComposeColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.ar.core.ArCoreApk
import com.google.ar.core.Config
import com.google.ar.core.Session
import com.google.ar.core.exceptions.UnavailableException
import dev.plume.app.ui.PlumeColors
import dev.plume.app.ui.PlumeGlyph
import dev.plume.app.ui.PlumeIcon
import dev.plume.app.ui.PlumeMark
import dev.plume.app.ui.PlumeTheme
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.plume_ffi.PlumeSession
import uniffi.plume_ffi.SceneMesh
import java.io.File

class ArActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }

        setContent {
            PlumeTheme {
                ArScreen(onClose = { finish() })
            }
        }
    }
}

private enum class ArPhase {
    Permission,
    Checking,
    Installing,
    Unsupported,
    Loading,
    Ready,
    Failed,
}

@Composable
private fun ArScreen(onClose: () -> Unit) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    var permissionGranted by remember {
        mutableStateOf(
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                PackageManager.PERMISSION_GRANTED,
        )
    }
    var permissionRequested by remember { mutableStateOf(false) }
    var phase by remember { mutableStateOf(ArPhase.Checking) }
    var message by remember { mutableStateOf("Preparing the camera") }
    var status by remember { mutableStateOf("Move your phone to find a surface") }
    var progress by remember { mutableIntStateOf(0) }
    var session by remember { mutableStateOf<Session?>(null) }
    var meshes by remember { mutableStateOf<List<SceneMesh>?>(null) }
    var installRequested by remember { mutableStateOf(false) }

    val permissionLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { granted ->
        permissionGranted = granted
        if (!granted) {
            phase = ArPhase.Permission
            message = context.getString(R.string.camera_permission_rationale)
        }
    }

    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        permissionGranted = ContextCompat.checkSelfPermission(
            context,
            Manifest.permission.CAMERA,
        ) == PackageManager.PERMISSION_GRANTED
        progress += 1
    }

    LaunchedEffect(permissionGranted, permissionRequested) {
        if (!permissionGranted && !permissionRequested) {
            permissionRequested = true
            permissionLauncher.launch(Manifest.permission.CAMERA)
        }
    }

    LaunchedEffect(permissionGranted, progress) {
        if (!permissionGranted) {
            phase = ArPhase.Permission
            message = context.getString(R.string.camera_permission_rationale)
            return@LaunchedEffect
        }
        if (session != null) return@LaunchedEffect

        phase = ArPhase.Checking
        message = "Checking AR support"
        val availability = runCatching {
            ArCoreApk.getInstance().checkAvailability(context)
        }.getOrElse { failure ->
            phase = ArPhase.Failed
            message = "AR support could not be checked: ${failure.message ?: "unknown error"}"
            return@LaunchedEffect
        }

        when (availability) {
            ArCoreApk.Availability.SUPPORTED_INSTALLED -> {
                phase = ArPhase.Loading
                message = "Building your sketch for the room"
                val created = runCatching {
                    Session(context).also { arSession ->
                        val config = Config(arSession).apply {
                            planeFindingMode = Config.PlaneFindingMode.HORIZONTAL_AND_VERTICAL
                            lightEstimationMode = Config.LightEstimationMode.AMBIENT_INTENSITY
                            focusMode = Config.FocusMode.AUTO
                            updateMode = Config.UpdateMode.LATEST_CAMERA_IMAGE
                        }
                        arSession.configure(config)
                    }
                }.getOrElse { failure ->
                    phase = ArPhase.Failed
                    message = if (failure is UnavailableException) {
                        "This device cannot start ARCore: ${failure.message}"
                    } else {
                        "The camera could not start: ${failure.message ?: "unknown error"}"
                    }
                    return@LaunchedEffect
                }

                try {
                    val loaded = runCatching {
                        withContext(Dispatchers.IO) { loadSceneMeshes(context) }
                    }.getOrElse { failure ->
                        phase = ArPhase.Failed
                        message = "Your sketch could not be packed for AR: " +
                            (failure.message ?: "unknown error")
                        return@LaunchedEffect
                    }
                    if (loaded.isEmpty()) {
                        phase = ArPhase.Failed
                        message = "This scene has no three-dimensional strokes yet. " +
                            "Draw a stroke in the studio, then try AR again."
                        return@LaunchedEffect
                    }
                    meshes = loaded
                    session = created
                    phase = ArPhase.Ready
                    status = "Tap a surface to place your sketch"
                } finally {
                    if (session !== created) {
                        runCatching { created.close() }
                    }
                }
            }

            ArCoreApk.Availability.SUPPORTED_NOT_INSTALLED,
            ArCoreApk.Availability.SUPPORTED_APK_TOO_OLD,
            -> {
                phase = ArPhase.Installing
                message = context.getString(R.string.ar_install_required)
                if (!installRequested && activity != null) {
                    installRequested = true
                    runCatching {
                        ArCoreApk.getInstance().requestInstall(activity, true)
                    }.onFailure {
                        phase = ArPhase.Failed
                        message = "Google Play Services for AR could not be opened."
                    }
                }
            }

            ArCoreApk.Availability.UNSUPPORTED_DEVICE_NOT_CAPABLE -> {
                phase = ArPhase.Unsupported
                message = context.getString(R.string.ar_unsupported) +
                    " Export a USDZ or GLB to preview it elsewhere."
            }

            ArCoreApk.Availability.UNKNOWN_CHECKING -> {
                phase = ArPhase.Checking
                message = "AR is still initializing. This usually takes a moment."
            }

            ArCoreApk.Availability.UNKNOWN_TIMED_OUT,
            ArCoreApk.Availability.UNKNOWN_ERROR,
            -> {
                phase = ArPhase.Failed
                message = "AR support could not be confirmed. Check your connection and try again."
            }
        }
    }

    Box(modifier = Modifier.fillMaxSize().background(ComposeColor.Black)) {
        val activeSession = session
        val activeMeshes = meshes
        if (phase == ArPhase.Ready && activeSession != null && activeMeshes != null) {
            ArSurface(
                session = activeSession,
                meshes = activeMeshes,
                onStatus = { status = it },
                modifier = Modifier.fillMaxSize(),
            )
        } else {
            ArBackdrop()
            ArStateCard(
                phase = phase,
                message = message,
                permissionGranted = permissionGranted,
                installRequested = installRequested,
                onAction = {
                    when {
                        !permissionGranted -> {
                            permissionRequested = true
                            permissionLauncher.launch(Manifest.permission.CAMERA)
                        }
                        phase == ArPhase.Installing && activity != null -> {
                            installRequested = false
                            progress += 1
                        }
                        else -> progress += 1
                    }
                },
                onClose = onClose,
            )
        }

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .statusBarsPadding()
                .padding(horizontal = 12.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            PlumeMark(modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(9.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    text = "PLACE IN YOUR SPACE",
                    style = MaterialTheme.typography.labelSmall,
                    color = PlumeColors.Ivory,
                    letterSpacing = 2.sp,
                    fontWeight = FontWeight.SemiBold,
                )
                Text(
                    text = "PLUME / AR",
                    style = MaterialTheme.typography.labelSmall,
                    color = PlumeColors.Muted,
                    letterSpacing = 1.sp,
                )
            }
            Surface(
                onClick = onClose,
                shape = CircleShape,
                color = PlumeColors.Ink.copy(alpha = 0.68f),
                contentColor = PlumeColors.Ivory,
                modifier = Modifier.size(38.dp),
            ) {
                Box(contentAlignment = Alignment.Center) {
                    PlumeIcon(
                        glyph = PlumeGlyph.Close,
                        label = "Close AR",
                        modifier = Modifier.size(18.dp),
                        tint = PlumeColors.Ivory,
                    )
                }
            }
        }

        if (phase == ArPhase.Ready) {
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .navigationBarsPadding()
                    .background(
                        Brush.verticalGradient(
                            listOf(
                                ComposeColor.Transparent,
                                PlumeColors.Ink.copy(alpha = 0.76f),
                            ),
                        ),
                    )
                    .padding(horizontal = 16.dp, vertical = 14.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Row(
                    modifier = Modifier
                        .clipPill()
                        .background(PlumeColors.Ink.copy(alpha = 0.72f))
                        .padding(horizontal = 12.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        modifier = Modifier
                            .size(5.dp)
                            .background(PlumeColors.Teal, CircleShape),
                    )
                    Spacer(Modifier.width(7.dp))
                    Text(
                        text = status,
                        style = MaterialTheme.typography.labelSmall,
                        color = PlumeColors.Ivory,
                        maxLines = 1,
                    )
                }
                Spacer(Modifier.height(7.dp))
                Text(
                    text = "Tap a surface to place  ·  pinch to resize  ·  double-tap to turn",
                    style = MaterialTheme.typography.labelSmall,
                    color = PlumeColors.Muted,
                    textAlign = TextAlign.Center,
                )
            }
        }
    }
}

@Composable
private fun ArSurface(
    session: Session,
    meshes: List<SceneMesh>,
    onStatus: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    val view = remember(session, meshes) {
        ArGLView(
            context,
            ArRenderer(session, meshes, onStatus) { context.displayRotationCompat() },
        ).apply {
            setEGLContextClientVersion(2)
            preserveEGLContextOnPause = true
            setEGLConfigChooser(8, 8, 8, 8, 16, 0)
            renderMode = GLSurfaceView.RENDERMODE_CONTINUOUSLY
        }
    }

    DisposableEffect(view, lifecycleOwner) {
        var resumed = false

        fun resumeAr() {
            if (resumed) return
            runCatching { session.resume() }
                .onSuccess {
                    resumed = true
                    view.onResume()
                }
                .onFailure {
                    onStatus("The camera could not resume: ${it.message}")
                }
        }

        resumeAr()

        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> resumeAr()
                Lifecycle.Event.ON_PAUSE -> {
                    if (resumed) {
                        runCatching { session.pause() }
                        resumed = false
                    }
                    view.onPause()
                }
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)

        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
            if (resumed) {
                runCatching { session.pause() }
            }
            view.onPause()
            runCatching { session.close() }
        }
    }

    AndroidView(
        factory = { view },
        modifier = modifier,
    )
}

@Composable
private fun ArBackdrop() {
    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(
                Brush.radialGradient(
                    colors = listOf(PlumeColors.PanelHigh, PlumeColors.Ink),
                ),
            ),
    )
}

@Composable
private fun ArStateCard(
    phase: ArPhase,
    message: String,
    permissionGranted: Boolean,
    installRequested: Boolean,
    onAction: () -> Unit,
    onClose: () -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(horizontal = 32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Surface(
            color = PlumeColors.Panel.copy(alpha = 0.94f),
            shape = RoundedCornerShape(20.dp),
            border = androidx.compose.foundation.BorderStroke(1.dp, PlumeColors.Line),
            modifier = Modifier.widthIn(max = 340.dp),
        ) {
            Column(
                modifier = Modifier.padding(22.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                if (phase == ArPhase.Loading || phase == ArPhase.Checking) {
                    CircularProgressIndicator(
                        modifier = Modifier.size(26.dp),
                        color = PlumeColors.Coral,
                        strokeWidth = 2.dp,
                    )
                } else {
                    PlumeIcon(
                        glyph = PlumeGlyph.Ar,
                        label = "AR placement",
                        modifier = Modifier.size(34.dp),
                        tint = if (phase == ArPhase.Failed || phase == ArPhase.Unsupported) {
                            PlumeColors.Muted
                        } else {
                            PlumeColors.Teal
                        },
                    )
                }
                Spacer(Modifier.height(14.dp))
                Text(
                    text = when (phase) {
                        ArPhase.Permission -> "Camera access"
                        ArPhase.Checking -> "Waking up AR"
                        ArPhase.Installing -> "One more install"
                        ArPhase.Unsupported -> "AR is not available here"
                        ArPhase.Loading -> "Bringing your sketch through"
                        ArPhase.Failed -> "Placement could not start"
                        ArPhase.Ready -> "Aim at the room"
                    },
                    style = MaterialTheme.typography.titleLarge,
                    color = PlumeColors.Ivory,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(8.dp))
                Text(
                    text = message,
                    style = MaterialTheme.typography.bodyMedium,
                    color = PlumeColors.Muted,
                    textAlign = TextAlign.Center,
                )
                Spacer(Modifier.height(18.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Surface(
                        onClick = onClose,
                        color = PlumeColors.PanelHigh,
                        contentColor = PlumeColors.Muted,
                        shape = RoundedCornerShape(10.dp),
                    ) {
                        Text(
                            text = "Back to studio",
                            modifier = Modifier.padding(horizontal = 14.dp, vertical = 9.dp),
                            style = MaterialTheme.typography.labelLarge,
                        )
                    }
                    when {
                        !permissionGranted -> {
                            ArAction("Allow camera", onAction)
                        }

                        phase == ArPhase.Installing -> {
                            ArAction(
                                if (installRequested) "Check again" else "Install AR support",
                                onAction,
                            )
                        }

                        phase == ArPhase.Failed || phase == ArPhase.Unsupported -> {
                            ArAction("Try again", onAction)
                        }

                        phase == ArPhase.Ready -> Unit
                        else -> Unit
                    }
                }
            }
        }
    }
}

@Composable
private fun ArAction(label: String, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        color = PlumeColors.Coral,
        contentColor = PlumeColors.Ink,
        shape = RoundedCornerShape(10.dp),
    ) {
        Text(
            text = label,
            modifier = Modifier.padding(horizontal = 14.dp, vertical = 9.dp),
            style = MaterialTheme.typography.labelLarge,
        )
    }
}

private fun Modifier.clipPill(): Modifier = clip(RoundedCornerShape(50))

@SuppressLint("ViewConstructor")
private class ArGLView(
    context: Context,
    private val renderer: ArRenderer,
) : GLSurfaceView(context) {
    private val tapDetector = GestureDetector(
        context,
        object : GestureDetector.SimpleOnGestureListener() {
            override fun onDown(event: MotionEvent): Boolean = true

            override fun onSingleTapConfirmed(event: MotionEvent): Boolean {
                performClick()
                queueEvent { renderer.requestPlacement(event.x, event.y) }
                return true
            }

            override fun onDoubleTap(event: MotionEvent): Boolean {
                queueEvent { renderer.turnQuarter() }
                return true
            }
        },
    )

    private val scaleDetector = ScaleGestureDetector(
        context,
        object : ScaleGestureDetector.SimpleOnScaleGestureListener() {
            override fun onScale(detector: ScaleGestureDetector): Boolean {
                val factor = detector.scaleFactor
                queueEvent { renderer.multiplyScale(factor) }
                return true
            }
        },
    )

    init {
        setRenderer(renderer)
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        tapDetector.onTouchEvent(event)
        scaleDetector.onTouchEvent(event)
        return true
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }
}

private fun loadSceneMeshes(context: Context): List<SceneMesh> {
    val core = PlumeSession()
    return try {
        val autosave = File(context.filesDir, "autosave.plume.json")
        val restored = if (autosave.isFile) {
            runCatching { core.loadDocumentJson(autosave.readText()) }.isSuccess
        } else {
            false
        }
        if (!restored) {
            core.loadGallery("ribbon-bloom")
        }
        core.sceneMeshes("draft")
    } finally {
        runCatching { core.close() }
    }
}

private fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
