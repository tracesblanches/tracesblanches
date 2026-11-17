package fr.skimap.app

import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.res.Configuration
import android.os.Bundle
import android.os.StrictMode
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import fr.skimap.app.community.RustCommunityCore
import fr.skimap.app.data.Appearance
import fr.skimap.app.data.AppearanceSetting
import fr.skimap.app.data.AssetStyleSource
import fr.skimap.app.data.PlatformLocator
import fr.skimap.app.data.PreferencesSettingsStore
import fr.skimap.app.data.RustDomainCatalog
import fr.skimap.app.data.RustPackRepository
import fr.skimap.app.data.UiChoices
import fr.skimap.app.day.DayActivity
import fr.skimap.app.guidance.AppSportActivity
import fr.skimap.app.sync.PackSync
import fr.skimap.app.sync.WorkManagerDownloads
import fr.skimap.app.ui.MapUiState
import fr.skimap.app.ui.MapViewModel
import fr.skimap.app.ui.SkimapApp
import fr.skimap.app.ui.domains.DomainsViewModel
import fr.skimap.app.ui.map.MapEngine
import fr.skimap.app.ui.theme.SkimapTheme
import fr.skimap.community.Community
import fr.skimap.sport.SportActivity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/**
 * The only activity. Packs live in the app-specific external storage,
 * `/sdcard/Android/data/fr.skimap.app/files/packs/<site>/`, downloaded from the server
 * (K2) or copied by `adb push` (README). Extra [EXTRA_PACK] (directory name) opens a
 * given pack.
 */
class MainActivity : ComponentActivity() {
    private val viewModel: MapViewModel by viewModels {
        factory(applicationContext, intent.getStringExtra(EXTRA_PACK), isDark(resources.configuration))
    }
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
        Community.core = Community.core ?: RustCommunityCore(applicationContext) { packsDir(applicationContext) }
        val settings = PreferencesSettingsStore { getSharedPreferences("skimap", Context.MODE_PRIVATE) }
        var autoUpdate by mutableStateOf(true)
        val choices = UiChoices { getSharedPreferences("skimap", Context.MODE_PRIVATE) }
        // The theme itself is kept by the system; the menu shows the stored choice.
        var appearance by mutableStateOf<Appearance?>(null)
        var welcome by mutableStateOf(false)
        val replayWelcome = intent.getBooleanExtra(EXTRA_WELCOME, false)
        lifecycleScope.launch {
            welcome = replayWelcome || !withContext(Dispatchers.IO) { choices.welcomeDone() }
            if (AppearanceSetting.supported) appearance = withContext(Dispatchers.IO) { choices.appearance() }
            autoUpdate = withContext(Dispatchers.IO) { settings.autoUpdate() }
            // The daily update, with its constraints (KEEP: scheduled once, not per launch).
            PackSync.schedulePeriodic(applicationContext, autoUpdate)
            // The France basemap (X20): the map of the whole country, before any pack.
            PackSync.downloadFrance(applicationContext)
            // The voice follows the phone's language (VO6): one download when it changed.
            withContext(Dispatchers.IO) {
                PackSync.followLanguage(applicationContext, PackSync.catalog(applicationContext).cached())
            }
            // Preparation of the next days on charge and Wi-Fi (E2), the less exposed terrain layer (V7).
            fr.skimap.app.prep.PrepWork
                .schedule(applicationContext)
            fr.skimap.app.rules.LowExposure
                .load(applicationContext)
        }
        setContent {
            SkimapTheme {
                SkimapApp(viewModel, domains, autoUpdate = autoUpdate, onAutoUpdate = { on ->
                    autoUpdate = on
                    lifecycleScope.launch {
                        withContext(Dispatchers.IO) { settings.setAutoUpdate(on) }
                        PackSync.schedulePeriodic(applicationContext, on)
                    }
                }, appearance = appearance, onAppearance = { a ->
                    appearance = a
                    lifecycleScope.launch {
                        withContext(Dispatchers.IO) { choices.setAppearance(a) }
                        // The activity restarts in the new theme (configuration change).
                        AppearanceSetting.apply(applicationContext, a)
                    }
                }, onPrepareDay = ::openDay, welcome = welcome, onWelcomeDone = {
                    welcome = false
                    lifecycleScope.launch(Dispatchers.IO) { choices.setWelcomeDone() }
                }, onSendSettings = { SportActivity.open(this, AppSportActivity::class.java) })
            }
        }
    }

    /** The morning of the area shown (« Préparer ma journée »). */
    private fun openDay() {
        val site = (viewModel.state.value as? MapUiState.Ready)?.opened?.info?.siteId
        startActivity(Intent(this, DayActivity::class.java).putExtra(DayActivity.EXTRA_SITE, site))
    }

    companion object {
        /** Name of the pack directory to open (site identifier), under [packsDir]; `libre`: free mode. */
        const val EXTRA_PACK = "pack"

        /** `true`: shows the welcome of the first launch again (nothing else is reset). */
        const val EXTRA_WELCOME = "welcome"

        /** Where packs are stored. */
        fun packsDir(context: Context): File = PackSync.packsDir(context)

        private fun isDark(c: Configuration) = c.uiMode and Configuration.UI_MODE_NIGHT_MASK == Configuration.UI_MODE_NIGHT_YES

        private fun domainsFactory(context: Context): ViewModelProvider.Factory =
            viewModelFactory {
                initializer {
                    DomainsViewModel(
                        catalog = RustDomainCatalog({ context.assets.open("catalog.json").use { it.readBytes().decodeToString() } }),
                        packs = RustPackRepository({ packsDir(context) }, franceDir = { PackSync.franceDir(context) }),
                        locator = PlatformLocator(context),
                        downloads = WorkManagerDownloads(context),
                    )
                }
            }

        private fun factory(
            context: Context,
            pack: String?,
            dark: Boolean,
        ): ViewModelProvider.Factory =
            viewModelFactory {
                initializer {
                    MapViewModel(
                        repository = RustPackRepository({ packsDir(context) }, franceDir = { PackSync.franceDir(context) }),
                        settings = PreferencesSettingsStore { context.getSharedPreferences("skimap", Context.MODE_PRIVATE) },
                        styles = AssetStyleSource({ context.assets }),
                        preferredPack = pack,
                        dark = dark,
                        downloads = WorkManagerDownloads(context),
                    )
                }
            }
    }
}
