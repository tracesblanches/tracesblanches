package fr.skimap.benchmark

import androidx.benchmark.macro.BaselineProfileMode
import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.StartupTimingMetric
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Cold start until the map of the pack is fully drawn (`reportFullyDrawn`: pack listed and
 * opened by the Rust core, style composed, MapLibre first full frame), without and with
 * the Baseline Profile. `timeToFullDisplayMs` is the whole job; `timeToInitialDisplayMs`
 * the first frame.
 */
@RunWith(AndroidJUnit4::class)
class StartupBenchmark {
    @get:Rule
    val rule = MacrobenchmarkRule()

    private fun startup(mode: CompilationMode) =
        rule.measureRepeated(
            packageName = PACKAGE,
            metrics = listOf(StartupTimingMetric()),
            compilationMode = mode,
            startupMode = StartupMode.COLD,
            iterations = ITERATIONS,
            setupBlock = { installPack() },
        ) {
            startOnPackAndWaitForMap()
        }

    @Test
    fun coldStartNoProfile() = startup(CompilationMode.None())

    @Test
    fun coldStartBaselineProfile() = startup(CompilationMode.Partial(BaselineProfileMode.Require))

    private companion object {
        const val ITERATIONS = 10
    }
}
