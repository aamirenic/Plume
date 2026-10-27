package dev.plume.app.ar

import android.content.Context
import android.opengl.GLES11Ext
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import android.opengl.Matrix
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Surface
import android.view.WindowManager
import com.google.ar.core.Anchor
import com.google.ar.core.Coordinates2d
import com.google.ar.core.Frame
import com.google.ar.core.Plane
import com.google.ar.core.Point
import com.google.ar.core.Session
import com.google.ar.core.TrackingState
import uniffi.plume_ffi.SceneMesh
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import kotlin.math.max

/**
 * Draws the ARCore camera as a full-screen external texture, overlays the
 * detected point cloud, and places PLUME's packed meshes on the first tapped
 * surface.  Everything here is purpose-built on OpenGL ES 2.0; no Sceneform or
 * third-party AR rendering library is involved.
 */
internal class ArRenderer(
    private val session: Session,
    meshes: List<SceneMesh>,
    private val onStatus: (String) -> Unit,
    private val displayRotation: () -> Int,
) : GLSurfaceView.Renderer {

    private val mainHandler = Handler(Looper.getMainLooper())
    private val renderMeshes = meshes.mapNotNull { prepareMesh(it) }
    private val totalTriangles = renderMeshes.sumOf { mesh -> mesh.chunks.sumOf { it.vertexCount / 3 } }

    private val bounds = computeBounds(meshes)
    private val fitScale = run {
        val extent = max(
            bounds.maxX - bounds.minX,
            max(bounds.maxY - bounds.minY, bounds.maxZ - bounds.minZ),
        )
        0.30f / extent.coerceAtLeast(0.001f)
    }

    private var backgroundColorProgram = 0
    private var meshProgram = 0
    private var pointProgram = 0
    private var cameraTexture = 0
    private var programFailure: String? = null
    private var meshBuffersReady = false

    private var backgroundPosition = 0
    private var backgroundTexCoord = 0
    private var backgroundSampler = 0

    private var meshPosition = 0
    private var meshNormal = 0
    private var meshMvp = 0
    private var meshModel = 0
    private var meshColor = 0
    private var meshEmission = 0

    private var pointPosition = 0
    private var pointMvp = 0
    private var pointColor = 0

    private var viewportWidth = 1
    private var viewportHeight = 1

    private val projection = FloatArray(16)
    private val view = FloatArray(16)
    private val viewProjection = FloatArray(16)
    private val anchorMatrix = FloatArray(16)
    private val adjustment = FloatArray(16)
    private val model = FloatArray(16)
    private val mvp = FloatArray(16)

    private val quadPositions = directFloatBuffer(
        floatArrayOf(-1f, -1f, 1f, -1f, -1f, 1f, 1f, 1f),
    )
    private val quadTexCoords = directFloatBuffer(FloatArray(8))
    private var anchor: Anchor? = null
    private var lastStatus = ""

    @Volatile
    private var pendingTap: FloatArray? = null

    @Volatile
    private var scaleMultiplier = 1f

    @Volatile
    private var rotationDegrees = 0f

    fun requestPlacement(x: Float, y: Float) {
        pendingTap = floatArrayOf(x, y)
    }

    fun multiplyScale(factor: Float) {
        if (!factor.isFinite() || factor <= 0f) return
        scaleMultiplier = (scaleMultiplier * factor).coerceIn(0.35f, 3.2f)
        report("Sketch resized · pinch to fine-tune")
    }

    fun turnQuarter() {
        rotationDegrees = (rotationDegrees + 45f) % 360f
        report("Sketch turned 45° · tap to reposition")
    }

    override fun onSurfaceCreated(gl: GL10?, config: EGLConfig?) {
        GLES20.glClearColor(0f, 0f, 0f, 1f)
        GLES20.glDisable(GLES20.GL_CULL_FACE)
        runCatching {
            buildPrograms()
            buildTexture()
            uploadMeshes()
            session.setCameraTextureName(cameraTexture)
            report("Move your phone slowly to find a surface")
        }.onFailure { failure ->
            programFailure = failure.message ?: "OpenGL setup failed"
            Log.e("PlumeAr", "Surface setup failed", failure)
            report("AR rendering could not start: $programFailure")
        }
    }

    override fun onSurfaceChanged(gl: GL10?, width: Int, height: Int) {
        viewportWidth = width.coerceAtLeast(1)
        viewportHeight = height.coerceAtLeast(1)
        GLES20.glViewport(0, 0, viewportWidth, viewportHeight)
        runCatching {
            session.setDisplayGeometry(
                displayRotation(),
                viewportWidth,
                viewportHeight,
            )
        }.onFailure {
            report("The camera display could not be sized: ${it.message}")
        }
    }

    override fun onDrawFrame(gl: GL10?) {
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT or GLES20.GL_DEPTH_BUFFER_BIT)
        if (programFailure != null) return

        val frame = runCatching { session.update() }.getOrElse {
            report("Waiting for the camera")
            return
        }

        drawBackground(frame)

        frame.camera.getProjectionMatrix(projection, 0, 0.08f, 60f)
        frame.camera.getViewMatrix(view, 0)

        drawPointCloud(frame)
        val tap = pendingTap
        pendingTap = null
        if (tap != null) {
            placeAt(tap[0], tap[1], frame)
        }
        drawAnchoredMeshes()
    }

    // -- background -----------------------------------------------------------

    private fun drawBackground(frame: Frame) {
        GLES20.glDisable(GLES20.GL_DEPTH_TEST)
        GLES20.glDepthMask(false)
        GLES20.glDisable(GLES20.GL_BLEND)
        GLES20.glUseProgram(backgroundColorProgram)

        GLES20.glActiveTexture(GLES20.GL_TEXTURE0)
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, cameraTexture)
        GLES20.glUniform1i(backgroundSampler, 0)

        runCatching {
            quadTexCoords.position(0)
            frame.transformCoordinates2d(
                Coordinates2d.OPENGL_NORMALIZED_DEVICE_COORDINATES,
                quadPositions,
                Coordinates2d.TEXTURE_NORMALIZED,
                quadTexCoords,
            )
        }.onFailure {
            // Keep the last good texture transform rather than dropping the
            // camera image for a frame.
            report("Camera image is still settling")
        }

        quadPositions.position(0)
        GLES20.glEnableVertexAttribArray(backgroundPosition)
        GLES20.glVertexAttribPointer(
            backgroundPosition,
            2,
            GLES20.GL_FLOAT,
            false,
            0,
            quadPositions,
        )
        quadTexCoords.position(0)
        GLES20.glEnableVertexAttribArray(backgroundTexCoord)
        GLES20.glVertexAttribPointer(
            backgroundTexCoord,
            2,
            GLES20.GL_FLOAT,
            false,
            0,
            quadTexCoords,
        )
        GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4)
        GLES20.glDisableVertexAttribArray(backgroundPosition)
        GLES20.glDisableVertexAttribArray(backgroundTexCoord)
    }

    // -- point cloud ----------------------------------------------------------

    private fun drawPointCloud(frame: Frame) {
        val cloud = runCatching { frame.acquirePointCloud() }.getOrNull() ?: return
        try {
            val points = cloud.points
            val count = points.remaining() / 4
            if (count <= 0) return
            GLES20.glDisable(GLES20.GL_DEPTH_TEST)
            GLES20.glDepthMask(false)
            GLES20.glEnable(GLES20.GL_BLEND)
            GLES20.glBlendFunc(GLES20.GL_SRC_ALPHA, GLES20.GL_ONE_MINUS_SRC_ALPHA)
            GLES20.glUseProgram(pointProgram)

            Matrix.multiplyMM(mvp, 0, projection, 0, view, 0)
            GLES20.glUniformMatrix4fv(pointMvp, 1, false, mvp, 0)
            GLES20.glUniform4f(pointColor, 0.36f, 0.80f, 0.80f, 0.62f)

            points.position(0)
            GLES20.glEnableVertexAttribArray(pointPosition)
            GLES20.glVertexAttribPointer(
                pointPosition,
                3,
                GLES20.GL_FLOAT,
                false,
                16,
                points,
            )
            GLES20.glDrawArrays(GLES20.GL_POINTS, 0, count)
            GLES20.glDisableVertexAttribArray(pointPosition)
            GLES20.glDisable(GLES20.GL_BLEND)
        } finally {
            cloud.release()
        }
    }

    // -- placement and meshes -------------------------------------------------

    private fun placeAt(x: Float, y: Float, frame: Frame) {
        val hit = frame.hitTest(x, y).firstOrNull { candidate ->
            when (val trackable = candidate.trackable) {
                is Plane -> trackable.trackingState == TrackingState.TRACKING &&
                    trackable.isPoseInPolygon(candidate.hitPose)
                is Point -> trackable.trackingState == TrackingState.TRACKING
                else -> false
            }
        }
        if (hit == null) {
            report("No surface there  ·  aim at a table or the floor")
            return
        }
        anchor?.detach()
        anchor = hit.createAnchor()
        report(
            "Placed $totalTriangles triangles  ·  pinch to resize  ·  double-tap to turn",
        )
    }

    private fun drawAnchoredMeshes() {
        val active = anchor
        if (active == null) {
            return
        }
        when (active.trackingState) {
            TrackingState.TRACKING -> Unit
            TrackingState.PAUSED -> return
            TrackingState.STOPPED -> {
                active.detach()
                anchor = null
                report("Surface lost  ·  tap to place it again")
                return
            }
        }
        if (!meshBuffersReady || renderMeshes.isEmpty()) return

        active.pose.toMatrix(anchorMatrix, 0)
        Matrix.setIdentityM(adjustment, 0)
        Matrix.rotateM(adjustment, 0, rotationDegrees, 0f, 1f, 0f)
        val scale = fitScale * scaleMultiplier
        Matrix.scaleM(adjustment, 0, scale, scale, scale)
        // The translation is post-multiplied, so it lands in model millimetres
        // after rotation and scale: R * S * T.
        Matrix.translateM(
            adjustment,
            0,
            -(bounds.minX + bounds.maxX) * 0.5f,
            -bounds.minY,
            -(bounds.minZ + bounds.maxZ) * 0.5f,
        )
        Matrix.multiplyMM(model, 0, anchorMatrix, 0, adjustment, 0)
        Matrix.multiplyMM(viewProjection, 0, projection, 0, view, 0)
        Matrix.multiplyMM(mvp, 0, viewProjection, 0, model, 0)

        GLES20.glEnable(GLES20.GL_DEPTH_TEST)
        GLES20.glDepthMask(true)
        GLES20.glDisable(GLES20.GL_BLEND)
        GLES20.glDisable(GLES20.GL_CULL_FACE)
        GLES20.glUseProgram(meshProgram)
        GLES20.glUniformMatrix4fv(meshMvp, 1, false, mvp, 0)
        GLES20.glUniformMatrix4fv(meshModel, 1, false, model, 0)

        for (mesh in renderMeshes) {
            GLES20.glUniform3f(meshColor, mesh.color[0], mesh.color[1], mesh.color[2])
            GLES20.glUniform1f(meshEmission, mesh.emission)
            for (chunk in mesh.chunks) {
                GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, chunk.positionBuffer)
                GLES20.glEnableVertexAttribArray(meshPosition)
                GLES20.glVertexAttribPointer(
                    meshPosition,
                    3,
                    GLES20.GL_FLOAT,
                    false,
                    0,
                    0,
                )
                GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, chunk.normalBuffer)
                GLES20.glEnableVertexAttribArray(meshNormal)
                GLES20.glVertexAttribPointer(
                    meshNormal,
                    3,
                    GLES20.GL_FLOAT,
                    false,
                    0,
                    0,
                )
                GLES20.glDrawArrays(GLES20.GL_TRIANGLES, 0, chunk.vertexCount)
            }
        }
        GLES20.glDisableVertexAttribArray(meshPosition)
        GLES20.glDisableVertexAttribArray(meshNormal)
        GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, 0)
    }

    // -- setup ----------------------------------------------------------------

    private fun buildPrograms() {
        backgroundColorProgram = linkProgram(BACKGROUND_VERTEX, BACKGROUND_FRAGMENT)
        meshProgram = linkProgram(MESH_VERTEX, MESH_FRAGMENT)
        pointProgram = linkProgram(POINT_VERTEX, POINT_FRAGMENT)

        backgroundPosition = GLES20.glGetAttribLocation(backgroundColorProgram, "a_Position")
        backgroundTexCoord = GLES20.glGetAttribLocation(backgroundColorProgram, "a_TexCoord")
        backgroundSampler = GLES20.glGetUniformLocation(backgroundColorProgram, "u_Texture")

        meshPosition = GLES20.glGetAttribLocation(meshProgram, "a_Position")
        meshNormal = GLES20.glGetAttribLocation(meshProgram, "a_Normal")
        meshMvp = GLES20.glGetUniformLocation(meshProgram, "u_Mvp")
        meshModel = GLES20.glGetUniformLocation(meshProgram, "u_Model")
        meshColor = GLES20.glGetUniformLocation(meshProgram, "u_Color")
        meshEmission = GLES20.glGetUniformLocation(meshProgram, "u_Emission")

        pointPosition = GLES20.glGetAttribLocation(pointProgram, "a_Position")
        pointMvp = GLES20.glGetUniformLocation(pointProgram, "u_Mvp")
        pointColor = GLES20.glGetUniformLocation(pointProgram, "u_Color")
    }

    private fun buildTexture() {
        val textures = IntArray(1)
        GLES20.glGenTextures(1, textures, 0)
        cameraTexture = textures[0]
        GLES20.glBindTexture(GLES11Ext.GL_TEXTURE_EXTERNAL_OES, cameraTexture)
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_MIN_FILTER,
            GLES20.GL_LINEAR,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_MAG_FILTER,
            GLES20.GL_LINEAR,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_WRAP_S,
            GLES20.GL_CLAMP_TO_EDGE,
        )
        GLES20.glTexParameteri(
            GLES11Ext.GL_TEXTURE_EXTERNAL_OES,
            GLES20.GL_TEXTURE_WRAP_T,
            GLES20.GL_CLAMP_TO_EDGE,
        )
    }

    private fun uploadMeshes() {
        for (mesh in renderMeshes) {
            for (chunk in mesh.chunks) {
                val ids = IntArray(2)
                GLES20.glGenBuffers(2, ids, 0)
                chunk.positionBuffer = ids[0]
                chunk.normalBuffer = ids[1]
                chunk.positionData.position(0)
                GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, chunk.positionBuffer)
                GLES20.glBufferData(
                    GLES20.GL_ARRAY_BUFFER,
                    chunk.positionData.capacity() * 4,
                    chunk.positionData,
                    GLES20.GL_STATIC_DRAW,
                )
                chunk.normalData.position(0)
                GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, chunk.normalBuffer)
                GLES20.glBufferData(
                    GLES20.GL_ARRAY_BUFFER,
                    chunk.normalData.capacity() * 4,
                    chunk.normalData,
                    GLES20.GL_STATIC_DRAW,
                )
            }
        }
        GLES20.glBindBuffer(GLES20.GL_ARRAY_BUFFER, 0)
        meshBuffersReady = true
    }

    private fun report(message: String) {
        if (message == lastStatus) return
        lastStatus = message
        mainHandler.post { onStatus(message) }
    }

    private fun linkProgram(vertexSource: String, fragmentSource: String): Int {
        val vertex = compileShader(GLES20.GL_VERTEX_SHADER, vertexSource)
        val fragment = compileShader(GLES20.GL_FRAGMENT_SHADER, fragmentSource)
        val program = GLES20.glCreateProgram()
        GLES20.glAttachShader(program, vertex)
        GLES20.glAttachShader(program, fragment)
        GLES20.glLinkProgram(program)
        val status = IntArray(1)
        GLES20.glGetProgramiv(program, GLES20.GL_LINK_STATUS, status, 0)
        if (status[0] == 0) {
            val log = GLES20.glGetProgramInfoLog(program)
            GLES20.glDeleteProgram(program)
            throw IllegalStateException("Program link failed: $log")
        }
        GLES20.glDeleteShader(vertex)
        GLES20.glDeleteShader(fragment)
        return program
    }

    private fun compileShader(type: Int, source: String): Int {
        val shader = GLES20.glCreateShader(type)
        GLES20.glShaderSource(shader, source)
        GLES20.glCompileShader(shader)
        val status = IntArray(1)
        GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, status, 0)
        if (status[0] == 0) {
            val log = GLES20.glGetShaderInfoLog(shader)
            GLES20.glDeleteShader(shader)
            throw IllegalStateException("Shader compile failed: $log")
        }
        return shader
    }

    private class RenderMesh(
        val color: FloatArray,
        val emission: Float,
        val chunks: List<RenderChunk>,
    )

    private class RenderChunk(
        val positionData: FloatBuffer,
        val normalData: FloatBuffer,
        val vertexCount: Int,
    ) {
        var positionBuffer: Int = 0
        var normalBuffer: Int = 0
    }

    private class FloatBuilder(capacity: Int) {
        var data = FloatArray(capacity.coerceAtLeast(9))
            private set
        var size = 0
            private set

        fun add(value: Float) {
            if (size == data.size) {
                data = data.copyOf(data.size * 2)
            }
            data[size++] = value
        }

        fun reset() {
            size = 0
        }

        fun toBuffer(): FloatBuffer {
            val direct = directFloatBuffer(data, size)
            reset()
            return direct
        }
    }

    private data class Bounds(
        val minX: Float,
        val minY: Float,
        val minZ: Float,
        val maxX: Float,
        val maxY: Float,
        val maxZ: Float,
    )

    private fun prepareMesh(mesh: SceneMesh): RenderMesh? {
        if (mesh.vertexCount <= 0u || mesh.indices.isEmpty()) return null
        val vertexCount = mesh.vertexCount.toInt()
        val positions = ByteBuffer.wrap(mesh.positions)
            .order(ByteOrder.LITTLE_ENDIAN)
            .asFloatBuffer()
        val normals = ByteBuffer.wrap(mesh.normals)
            .order(ByteOrder.LITTLE_ENDIAN)
            .asFloatBuffer()
        if (positions.capacity() < vertexCount * 3 || normals.capacity() < vertexCount * 3) {
            return null
        }
        val indexBuffer = ByteBuffer.wrap(mesh.indices)
            .order(ByteOrder.LITTLE_ENDIAN)
            .asIntBuffer()
        val triangles = indexBuffer.capacity() / 3
        if (triangles == 0) return null

        val maxVerticesPerChunk = 60_000
        val startCapacity = minOf(triangles * 9, maxVerticesPerChunk * 3)
        val positionBuilder = FloatBuilder(startCapacity)
        val normalBuilder = FloatBuilder(startCapacity)
        val chunks = ArrayList<RenderChunk>()
        var verticesInChunk = 0

        fun flush() {
            if (verticesInChunk == 0) return
            chunks += RenderChunk(
                positionData = positionBuilder.toBuffer(),
                normalData = normalBuilder.toBuffer(),
                vertexCount = verticesInChunk,
            )
            verticesInChunk = 0
        }

        for (triangle in 0 until triangles) {
            if (verticesInChunk + 3 > maxVerticesPerChunk) {
                flush()
            }
            val first = indexBuffer.get(triangle * 3)
            val second = indexBuffer.get(triangle * 3 + 1)
            val third = indexBuffer.get(triangle * 3 + 2)
            if (
                first < 0 || first >= vertexCount ||
                second < 0 || second >= vertexCount ||
                third < 0 || third >= vertexCount
            ) {
                continue
            }
            for (index in intArrayOf(first, second, third)) {
                val base = index * 3
                positionBuilder.add(positions.get(base))
                positionBuilder.add(positions.get(base + 1))
                positionBuilder.add(positions.get(base + 2))
                normalBuilder.add(normals.get(base))
                normalBuilder.add(normals.get(base + 1))
                normalBuilder.add(normals.get(base + 2))
            }
            verticesInChunk += 3
        }
        flush()
        if (chunks.isEmpty()) return null
        return RenderMesh(
            color = floatArrayOf(
                mesh.color.getOrElse(0) { 0.9f },
                mesh.color.getOrElse(1) { 0.9f },
                mesh.color.getOrElse(2) { 0.85f },
            ),
            emission = mesh.emission,
            chunks = chunks,
        )
    }

    private fun computeBounds(meshes: List<SceneMesh>): Bounds {
        var minX = Float.POSITIVE_INFINITY
        var minY = Float.POSITIVE_INFINITY
        var minZ = Float.POSITIVE_INFINITY
        var maxX = Float.NEGATIVE_INFINITY
        var maxY = Float.NEGATIVE_INFINITY
        var maxZ = Float.NEGATIVE_INFINITY
        for (mesh in meshes) {
            val buffer = ByteBuffer.wrap(mesh.positions)
                .order(ByteOrder.LITTLE_ENDIAN)
                .asFloatBuffer()
            val count = minOf(mesh.vertexCount.toInt(), buffer.capacity() / 3)
            for (i in 0 until count) {
                val x = buffer.get(i * 3)
                val y = buffer.get(i * 3 + 1)
                val z = buffer.get(i * 3 + 2)
                if (!x.isFinite() || !y.isFinite() || !z.isFinite()) continue
                minX = minOf(minX, x)
                minY = minOf(minY, y)
                minZ = minOf(minZ, z)
                maxX = maxOf(maxX, x)
                maxY = maxOf(maxY, y)
                maxZ = maxOf(maxZ, z)
            }
        }
        if (!minX.isFinite()) {
            return Bounds(-0.5f, 0f, -0.5f, 0.5f, 1f, 0.5f)
        }
        return Bounds(minX, minY, minZ, maxX, maxY, maxZ)
    }

    companion object {
        private const val BACKGROUND_VERTEX = """
            attribute vec4 a_Position;
            attribute vec2 a_TexCoord;
            varying vec2 v_TexCoord;
            void main() {
                gl_Position = a_Position;
                v_TexCoord = a_TexCoord;
            }
        """

        private const val BACKGROUND_FRAGMENT = """
            #extension GL_OES_EGL_image_external : require
            precision mediump float;
            uniform samplerExternalOES u_Texture;
            varying vec2 v_TexCoord;
            void main() {
                gl_FragColor = texture2D(u_Texture, v_TexCoord);
            }
        """

        private const val MESH_VERTEX = """
            attribute vec3 a_Position;
            attribute vec3 a_Normal;
            uniform mat4 u_Mvp;
            uniform mat4 u_Model;
            varying vec3 v_Normal;
            void main() {
                gl_Position = u_Mvp * vec4(a_Position, 1.0);
                v_Normal = normalize((u_Model * vec4(a_Normal, 0.0)).xyz);
            }
        """

        private const val MESH_FRAGMENT = """
            precision mediump float;
            uniform vec3 u_Color;
            uniform float u_Emission;
            varying vec3 v_Normal;
            void main() {
                vec3 normal = length(v_Normal) > 0.0001 ? normalize(v_Normal) : vec3(0.0, 1.0, 0.0);
                vec3 light = normalize(vec3(0.35, 0.82, 0.45));
                float front = max(dot(normal, light), 0.0);
                float rim = max(dot(normal, -light), 0.0);
                vec3 lit = u_Color * (0.34 + 0.78 * front + 0.18 * rim);
                lit += u_Color * u_Emission * 0.55;
                gl_FragColor = vec4(pow(lit, vec3(0.4545)), 1.0);
            }
        """

        private const val POINT_VERTEX = """
            attribute vec3 a_Position;
            uniform mat4 u_Mvp;
            void main() {
                gl_Position = u_Mvp * vec4(a_Position, 1.0);
                gl_PointSize = 6.0;
            }
        """

        private const val POINT_FRAGMENT = """
            precision mediump float;
            uniform vec4 u_Color;
            void main() {
                vec2 p = gl_PointCoord - vec2(0.5);
                if (length(p) > 0.5) discard;
                gl_FragColor = u_Color;
            }
        """
    }
}

private fun directFloatBuffer(values: FloatArray, count: Int = values.size): FloatBuffer {
    val buffer = ByteBuffer.allocateDirect(count * 4)
        .order(ByteOrder.nativeOrder())
        .asFloatBuffer()
    buffer.put(values, 0, count.coerceAtMost(values.size))
    buffer.position(0)
    return buffer
}

@Suppress("DEPRECATION")
internal fun Context.displayRotationCompat(): Int =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        display?.rotation ?: Surface.ROTATION_0
    } else {
        (getSystemService(Context.WINDOW_SERVICE) as WindowManager).defaultDisplay.rotation
    }
