package dev.plume.app

import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import dev.plume.app.ui.EditorScreen
import dev.plume.app.ui.PlumeTheme

class MainActivity : ComponentActivity() {
    private val viewModel: PlumeViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            window.isNavigationBarContrastEnforced = false
        }

        setContent {
            PlumeTheme {
                EditorScreen(viewModel = viewModel)
            }
        }
    }

    override fun onStop() {
        // Autosave is debounced while editing; flush synchronously when the app
        // leaves the foreground so a process kill cannot lose the last stroke.
        viewModel.saveAutosaveNow()
        super.onStop()
    }
}
