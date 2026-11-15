import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.ListProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.CacheableTask
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import javax.inject.Inject

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.roborazzi)
    alias(libs.plugins.baselineprofile)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

// ---------------------------------------------------------------------------------------
// Rust core (crates/ffi, UniFFI): built by cargo from the repository root, never versioned.

/** Repository root: the Cargo workspace. */
val repoRoot: File = rootDir.parentFile

/** Sources that change the Rust library: the workspace crates, manifests and lock file. */
val rustSources =
    files(
        fileTree(repoRoot.resolve("crates")) {
            include("**/*.rs", "**/Cargo.toml", "**/uniffi.toml")
            exclude("**/target/**")
        },
        fileTree(repoRoot.resolve("vendor")) { include("**/*.rs", "**/Cargo.toml") },
        repoRoot.resolve("Cargo.toml"),
        repoRoot.resolve("Cargo.lock"),
        repoRoot.resolve("rust-toolchain.toml"),
    )

/** Runs `cargo` in the workspace and copies its products into [outputDir]. */
abstract class CargoTask : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sources: ConfigurableFileCollection

    @get:Internal
    abstract val workspace: DirectoryProperty

    @get:Input
    abstract val arguments: ListProperty<String>

    @get:Input
    abstract val environment: org.gradle.api.provider.MapProperty<String, String>

    /**
     * Files copied into [outputDir] after the build: `source` (relative to the workspace,
     * copied under its own name) or `source=>destination` (relative to [outputDir]).
     */
    @get:Input
    abstract val products: ListProperty<String>

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:Inject
    abstract val exec: ExecOperations

    @TaskAction
    fun run() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        exec.exec {
            workingDir = workspace.get().asFile
            executable = "cargo"
            args(arguments.get().map { it.replace("@OUT@", out.absolutePath) })
            environment(this@CargoTask.environment.get())
        }
        products.get().forEach { product ->
            val (from, to) = product.split("=>").let { it[0] to it.getOrNull(1) }
            val src = workspace.get().asFile.resolve(from)
            val dest = out.resolve(to ?: src.name)
            src.copyTo(dest, overwrite = true)
            if (src.canExecute()) dest.setExecutable(true)
        }
    }
}

/**
 * Copies the map styles validated by the user (the JSON files of `styles/`, K15 #123) into the assets
 * of the app, unchanged: `assets/styles/<style>-<theme>.json`. The app instantiates them
 * on the packs through the Rust core; it has no style of its own.
 */
@CacheableTask
abstract class StyleAssetsTask : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val styles: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @TaskAction
    fun run() {
        val out = outputDir.get().asFile.resolve("styles")
        outputDir.get().asFile.deleteRecursively()
        out.mkdirs()
        val copied = styles.files.filter { it.name != "pack-schema.json" }
        // fond, srn, guidage, each light (clair) and dark (sombre).
        check(copied.size == 6) { "expected 6 styles in styles/, found ${copied.map { it.name }}" }
        copied.forEach { it.copyTo(out.resolve(it.name), overwrite = true) }
    }
}

/**
 * Publishes packs, in order, into a fresh skimap-server store (`skimap-server publish`):
 * the store the download tests serve (K2), as the production server would publish it.
 */
abstract class PublishTask : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val server: RegularFileProperty

    /** Pack directories, published in this order (versions 1 then 2: a delta). */
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val packs: ConfigurableFileCollection

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:Inject
    abstract val exec: ExecOperations

    @TaskAction
    fun run() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        exec.exec {
            executable = server.get().asFile.absolutePath
            args(listOf("publish", "--store", out.absolutePath) + packs.files.map { it.absolutePath })
        }
    }
}

/** Generates the Kotlin bindings of the Rust library (UniFFI, library mode). */
@CacheableTask
abstract class UniffiBindgenTask : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val library: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.NONE)
    abstract val bindgen: RegularFileProperty

    @get:Internal
    abstract val workspace: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:Inject
    abstract val exec: ExecOperations

    @TaskAction
    fun run() {
        val out = outputDir.get().asFile
        out.deleteRecursively()
        exec.exec {
            // Library mode reads crates/ffi/uniffi.toml through `cargo metadata`.
            workingDir = workspace.get().asFile
            executable = bindgen.get().asFile.absolutePath
            args(
                "generate",
                "--library",
                library.get().asFile.absolutePath,
                "--language",
                "kotlin",
                "--no-format",
                "--out-dir",
                out.absolutePath,
            )
        }
    }
}

/** ABIs of the app: Pixel 8 (arm64-v8a) and the x86_64 emulator. MapLibre and JNA ship
 * more; the filter keeps only those our Rust library is built for. */
val abis = listOf("arm64-v8a", "x86_64")

/** Rust target of each Android ABI. */
val rustTargets =
    mapOf(
        "arm64-v8a" to "aarch64-linux-android",
        "armeabi-v7a" to "armv7-linux-androideabi",
        "x86_64" to "x86_64-linux-android",
        "x86" to "i686-linux-android",
    )
val minSdkVersion = 29

android {
    namespace = "fr.skimap.app"
    compileSdk = 37
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "fr.skimap.app"
        minSdk = minSdkVersion
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // Each instrumented test in its own process (Test Orchestrator): no state shared
        // between tests (UiAutomation of the accessibility checks, map probe).
        testInstrumentationRunnerArguments["clearPackageData"] = "false"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    testOptions {
        execution = "ANDROIDX_TEST_ORCHESTRATOR"
        unitTests {
            isIncludeAndroidResources = true
            // android.util.Log in plain JVM tests (ViewModel): no-op.
            isReturnDefaultValues = true
        }
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
        checkDependencies = true
        lintConfig = file("lint.xml")
    }

    packaging {
        // JNA ships one libjnidispatch.so per ABI in its AAR; keep them uncompressed with ours.
        jniLibs.useLegacyPackaging = false
    }
}

val ndkDir = androidComponents.sdkComponents.ndkDirectory

val cargoNdk =
    tasks.register<CargoTask>("cargoNdk") {
        description = "Builds libskimap_ffi.so for ${abis.joinToString()} with cargo-ndk."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(
            listOf("ndk") + abis.flatMap { listOf("-t", it) } +
                listOf("--platform", minSdkVersion.toString()) +
                listOf("build", "--release", "--locked", "-p", "skimap-ffi"),
        )
        environment.set(ndkDir.map { mapOf("ANDROID_NDK_HOME" to it.asFile.absolutePath) })
        // Only our library: `cargo ndk -o` would also copy every other cdylib of the target
        // directory (proj4rs builds one, unused: it is linked statically).
        products.set(
            abis.map { abi ->
                val triple = rustTargets[abi] ?: error("ABI $abi has no Rust target")
                "target/$triple/release/libskimap_ffi.so=>$abi/libskimap_ffi.so"
            },
        )
        outputDir.set(layout.buildDirectory.dir("rust/jniLibs"))
    }

val cargoHost =
    tasks.register<CargoTask>("cargoHost") {
        description = "Builds libskimap_ffi.so for this machine (bindings, JVM tests)."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(listOf("build", "--release", "--locked", "-p", "skimap-ffi"))
        environment.set(emptyMap())
        products.set(listOf("target/release/libskimap_ffi.so"))
        outputDir.set(layout.buildDirectory.dir("rust/host"))
    }

val bindgenTool =
    tasks.register<CargoTask>("bindgenTool") {
        description = "Builds the UniFFI binding generator (android/uniffi-bindgen, its own Cargo project)."
        sources.from(fileTree(rootDir.resolve("uniffi-bindgen")) { include("src/**", "Cargo.toml", "Cargo.lock") })
        workspace.set(repoRoot)
        arguments.set(listOf("build", "--release", "--locked", "--manifest-path", "android/uniffi-bindgen/Cargo.toml"))
        environment.set(emptyMap())
        products.set(listOf("android/uniffi-bindgen/target/release/uniffi-bindgen"))
        outputDir.set(layout.buildDirectory.dir("rust/bindgen"))
    }

val uniffiBindings =
    tasks.register<UniffiBindgenTask>("uniffiBindings") {
        description = "Generates the Kotlin bindings of crates/ffi (UniFFI)."
        library.set(cargoHost.flatMap { it.outputDir.file("libskimap_ffi.so") })
        bindgen.set(bindgenTool.flatMap { it.outputDir.file("uniffi-bindgen") })
        workspace.set(repoRoot)
        outputDir.set(layout.buildDirectory.dir("generated/uniffi/kotlin"))
    }

val styleAssets =
    tasks.register<StyleAssetsTask>("styleAssets") {
        description = "Copies the validated map styles of styles/ into the app assets."
        styles.from(fileTree(repoRoot.resolve("styles")) { include("*.json") })
        outputDir.set(layout.buildDirectory.dir("generated/styleAssets"))
    }

val testPack =
    tasks.register<CargoTask>("testPack") {
        description = "Writes the synthetic test pack (crates/ffi/examples/test_pack.rs)."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(
            listOf("run", "--release", "--locked", "-p", "skimap-ffi", "--example", "test_pack", "--", "@OUT@/testpack"),
        )
        environment.set(emptyMap())
        products.set(emptyList())
        outputDir.set(layout.buildDirectory.dir("generated/testPack"))
    }

val cargoServer =
    tasks.register<CargoTask>("cargoServer") {
        description = "Builds skimap-server for this machine (download tests against the real server)."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(listOf("build", "--release", "--locked", "-p", "skimap-server"))
        environment.set(emptyMap())
        products.set(listOf("target/release/skimap-server"))
        outputDir.set(layout.buildDirectory.dir("rust/server"))
    }

val testPackV2 =
    tasks.register<CargoTask>("testPackV2") {
        description = "Writes version 2 of the test pack (a delta from version 1)."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(
            listOf("run", "--release", "--locked", "-p", "skimap-ffi", "--example", "test_pack", "--", "@OUT@/testpack", "2"),
        )
        environment.set(emptyMap())
        products.set(emptyList())
        outputDir.set(layout.buildDirectory.dir("generated/testPackV2"))
    }

val testVoicePack =
    tasks.register<CargoTask>("testVoicePack") {
        description = "Writes the English voice pack of the test site (VO6)."
        sources.from(rustSources)
        workspace.set(repoRoot)
        arguments.set(
            listOf("run", "--release", "--locked", "-p", "skimap-ffi", "--example", "test_pack", "--", "@OUT@/voice-en", "voice-en"),
        )
        environment.set(emptyMap())
        products.set(emptyList())
        outputDir.set(layout.buildDirectory.dir("generated/testVoicePack"))
    }

/** The store of the download tests: test pack v1 then v2, and the English voice pack. */
val testStore =
    tasks.register<PublishTask>("testStore") {
        description = "Publishes the test packs into a skimap-server store (build/generated/testStore)."
        server.set(cargoServer.flatMap { it.outputDir.file("skimap-server") })
        packs.from(
            testPack.map { it.outputDir.dir("testpack") },
            testPackV2.map { it.outputDir.dir("testpack") },
            testVoicePack.map { it.outputDir.dir("voice-en") },
        )
        outputDir.set(layout.buildDirectory.dir("generated/testStore"))
    }

androidComponents {
    onVariants { variant ->
        variant.sources.kotlin?.addGeneratedSourceDirectory(uniffiBindings, UniffiBindgenTask::outputDir)
        variant.sources.jniLibs?.addGeneratedSourceDirectory(cargoNdk, CargoTask::outputDir)
        variant.sources.assets?.addGeneratedSourceDirectory(styleAssets, StyleAssetsTask::outputDir)
        variant.deviceTests.values.forEach { test ->
            test.sources.assets?.addGeneratedSourceDirectory(testPack, CargoTask::outputDir)
        }
    }
}

tasks.withType<Test>().configureEach {
    val host = cargoHost.flatMap { it.outputDir }
    val pack = testPack.flatMap { it.outputDir }
    val store = testStore.flatMap { it.outputDir }
    val serverBin = cargoServer.flatMap { it.outputDir.file("skimap-server") }
    inputs.dir(host).withPathSensitivity(PathSensitivity.NONE)
    inputs.dir(pack).withPathSensitivity(PathSensitivity.RELATIVE)
    inputs.dir(store).withPathSensitivity(PathSensitivity.RELATIVE)
    inputs.file(serverBin).withPathSensitivity(PathSensitivity.NONE)
    inputs.dir(repoRoot.resolve("styles")).withPathSensitivity(PathSensitivity.RELATIVE)
    val hostPath = host.map { it.asFile.absolutePath }
    val packPath = pack.map { it.asFile.resolve("testpack").absolutePath }
    val storePath = store.map { it.asFile.absolutePath }
    val serverPath = serverBin.map { it.asFile.absolutePath }
    val stylesPath = repoRoot.resolve("styles").absolutePath
    jvmArgumentProviders.add(
        CommandLineArgumentProvider {
            listOf(
                "-Djna.library.path=${hostPath.get()}",
                "-Dskimap.testPack=${packPath.get()}",
                "-Dskimap.styles=$stylesPath",
                "-Dskimap.store=${storePath.get()}",
                "-Dskimap.server=${serverPath.get()}",
            )
        },
    )
}

composeCompiler {
    // The UniFFI records are immutable data classes: stable, so screens skip when equal.
    stabilityConfigurationFiles.add(rootProject.layout.projectDirectory.file("compose-stability.conf"))
}

// ---------------------------------------------------------------------------------------
// Static analysis.

detekt {
    buildUponDefaultConfig = true
    allRules = false
    config.setFrom(files("$rootDir/config/detekt/detekt.yml"))
    source.setFrom(files("src/main/kotlin", "src/test/kotlin", "src/androidTest/kotlin"))
}

ktlint {
    version.set(libs.versions.ktlint)
    android.set(true)
    filter {
        exclude { it.file.path.contains("/build/") }
    }
}

roborazzi {
    outputDir.set(file("src/test/screenshots"))
}

dependencies {
    implementation(project(":tracking"))
    // Sending the outing (K11): Health Connect and Strava.
    implementation(project(":sport"))
    // Reports, OSM notes, « Mes sorties » (V8 to V15).
    implementation(project(":community"))
    // No voice commands in the app for now (decision of 15/11/2026): the module :voicecmd
    // (V3, VO5, A11) stays in the repository and builds and tests on its own, out of the APK.
    // Group mode (N2): the BLE radio in the location service, the group screen.
    implementation(project(":group"))
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.maplibre.android)
    implementation(libs.jna) { artifact { type = "aar" } }
    // Pack download and update (K2): the network module and WorkManager.
    implementation(project(":sync"))
    implementation(libs.androidx.work.runtime)
    // Installs the Baseline Profile (benchmark module) on devices without Play.
    implementation(libs.androidx.profileinstaller)
    baselineProfile(project(":benchmark"))
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.jna)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.org.json)
    testImplementation(libs.robolectric)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(libs.roborazzi.junit.rule)
    testImplementation(libs.androidx.test.core)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)
    testImplementation(libs.compose.ui.test.junit4.accessibility)

    androidTestImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.core)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    // Espresso drives the Compose test idling; 3.5 (from ui-test) fails on Android 17.
    androidTestImplementation(libs.androidx.test.espresso.core)
    androidTestImplementation(libs.androidx.test.uiautomator)
    androidTestUtil(libs.androidx.test.orchestrator)
    androidTestUtil(libs.androidx.test.services)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.compose.ui.test.junit4.accessibility)

    detektPlugins(libs.detekt.compose.rules)
}
