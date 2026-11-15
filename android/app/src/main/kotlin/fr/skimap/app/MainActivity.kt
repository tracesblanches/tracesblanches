package fr.skimap.app

import android.content.Context
import android.content.pm.ApplicationInfo
import android.os.Bundle
import android.os.StrictMode
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import fr.skimap.app.data.PlatformLocator
import fr.skimap.app.data.PreferencesSettingsStore
import fr.skimap.app.data.RustDomainCatalog
import fr.skimap.app.data.RustPackRepository
import fr.skimap.app.ui.MapViewModel
import fr.skimap.app.ui.SkimapApp
import fr.skimap.app.ui.domains.DomainsViewModel
import fr.skimap.app.ui.map.MapEngine
import fr.skimap.app.ui.theme.SkimapTheme
import java.io.File

/**
 * The only activity. Packs live in the app-specific external storage,
 * `/sdcard/Android/data/fr.skimap.app/files/packs/<site>/`, where `adb push` can copy them
 * (README). Extra [EXTRA_PACK] (directory name) opens a given pack.
 */
class MainActivity : ComponentActivity() {
    private val viewModel: MapViewModel by viewModels { factory(applicationContext, intent.getStringExtra(EXTRA_PACK)) }
    private val domains: DomainsViewModel by viewModels { domainsFactory(applicationContext) }

    override fun onCreate(savedInstanceState: Bundle?) {
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
            // Debug builds log any disk or network access on the main thread.
            StrictMode.setThreadPolicy(
                StrictMode.ThreadPolicy
                    .Builder()
                    .detectDiskReads()
                    .detectDiskWrites()
                    .detectNetwork()
                    .penaltyLog()
                    .build(),
            )
        }
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        MapEngine.start(this, lifecycleScope)
        setContent {
            SkimapTheme {
                SkimapApp(viewModel, domains)
            }
        }
    }

    companion object {
        /** Name of the pack directory to open (`cervieres`), under [packsDir]; `libre`: free mode. */
        const val EXTRA_PACK = "pack"

        /** Where packs are stored. */
        fun packsDir(context: Context): File = File(context.getExternalFilesDir(null), "packs")

        private fun domainsFactory(context: Context): ViewModelProvider.Factory =
            viewModelFactory {
                initializer {
                    DomainsViewModel(
                        catalog = RustDomainCatalog({ context.assets.open("catalog.json").use { it.readBytes().decodeToString() } }),
                        packs = RustPackRepository({ packsDir(context) }),
                        locator = PlatformLocator(context),
                    )
                }
            }

        private fun factory(
            context: Context,
            pack: String?,
        ): ViewModelProvider.Factory =
            viewModelFactory {
                initializer {
                    val repository = RustPackRepository({ packsDir(context) })
                    MapViewModel(
                        repository = repository,
                        settings = PreferencesSettingsStore { context.getSharedPreferences("skimap", Context.MODE_PRIVATE) },
                        preferredPack = pack,
                    )
                }
            }
    }
}
