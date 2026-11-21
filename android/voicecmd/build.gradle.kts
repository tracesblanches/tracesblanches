import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.Optional
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import java.net.URI
import java.security.MessageDigest
import java.util.zip.ZipFile
import javax.inject.Inject

// Offline voice commands (V3 #66, VO5 #87, A11 #98): the speech stack of the speak project
// (sherpa-onnx 1.13.8, Silero VAD v6.2.3, Parakeet-TDT-0.6B-v3 int8), loaded on demand and
// released after each command. The model is an optional download (ModelSpec), never in
// the APK; only the two native libraries the JNI needs are. No UniFFI type here: the app
// adapts the core's classifier to CommandClassifier, so this module is tested on the JVM.
plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

/** sherpa-onnx release used by speak (app/DEPENDENCIES.md): AAR and host JNI, sha256-pinned. */
val sherpaVersion = "1.13.8"
val sherpaRelease = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v$sherpaVersion"
val sherpaAarSha = "633c24321e06b1fe79feafa03ea16cbc0f8a286641e2da3559bac91bdb13bd96"
val sherpaHostSha = "f18bf581b408d8c28539c1b28899b4d4811faebb74eede08db21fd6e9243e346"

/** Downloads (once, into ~/.cache/skimap) and checks a pinned file, then unpacks it. */
abstract class FetchTask : DefaultTask() {
    @get:Input abstract val url: Property<String>

    @get:Input abstract val sha256: Property<String>

    @get:Input abstract val cacheFile: Property<String>

    /** `aar`: classes.jar and the JNI libraries; `tar`: the host libraries (tar.bz2). */
    @get:Input abstract val kind: Property<String>

    @get:OutputDirectory abstract val outputDir: DirectoryProperty

    /** `aar`: the JNI libraries, by ABI (a jniLibs source directory of the variants). */
    @get:Optional @get:OutputDirectory
    abstract val jniDir: DirectoryProperty

    @get:Inject abstract val exec: ExecOperations

    @TaskAction
    fun run() {
        val f = File(cacheFile.get())
        if (!f.isFile || digest(f) != sha256.get()) {
            f.parentFile.mkdirs()
            val tmp = File(f.path + ".part")
            URI(url.get()).toURL().openStream().use { i -> tmp.outputStream().use { i.copyTo(it) } }
            val got = digest(tmp)
            check(got == sha256.get()) { "${url.get()}: sha256 $got, expected ${sha256.get()}" }
            tmp.renameTo(f)
        }
        val out = outputDir.get().asFile
        out.deleteRecursively()
        out.mkdirs()
        if (jniDir.isPresent) jniDir.get().asFile.deleteRecursively()
        when (kind.get()) {
            "aar" -> {
                ZipFile(f).use { z ->
                    z.getInputStream(z.getEntry("classes.jar")).use { i -> out.resolve("classes.jar").outputStream().use { i.copyTo(it) } }
                    // Only what libsherpa-onnx-jni.so needs (readelf: libonnxruntime.so), for the ABIs of the app.
                    for (abi in listOf("arm64-v8a", "x86_64")) {
                        for (lib in listOf("libsherpa-onnx-jni.so", "libonnxruntime.so")) {
                            val dest = jniDir.get().asFile.resolve("$abi/$lib")
                            dest.parentFile.mkdirs()
                            z.getInputStream(z.getEntry("jni/$abi/$lib")).use { i -> dest.outputStream().use { i.copyTo(it) } }
                        }
                    }
                }
            }

            else -> {
                exec.exec {
                    commandLine("tar", "-C", out.absolutePath, "--strip-components=1", "-xjf", f.absolutePath)
                }
            }
        }
    }

    private fun digest(f: File): String {
        val md = MessageDigest.getInstance("SHA-256")
        f.inputStream().use { i ->
            val buf = ByteArray(1 shl 16)
            while (true) {
                val n = i.read(buf)
                if (n < 0) break
                md.update(buf, 0, n)
            }
        }
        return md.digest().joinToString("") { "%02x".format(it) }
    }
}

val cacheDir = "${System.getProperty("user.home")}/.cache/skimap/sherpa-onnx"

val sherpaAar =
    tasks.register<FetchTask>("sherpaAar") {
        description = "sherpa-onnx $sherpaVersion AAR: Kotlin API and the JNI of arm64-v8a and x86_64."
        url.set("$sherpaRelease/sherpa-onnx-$sherpaVersion.aar")
        sha256.set(sherpaAarSha)
        cacheFile.set("$cacheDir/sherpa-onnx-$sherpaVersion.aar")
        kind.set("aar")
        outputDir.set(layout.buildDirectory.dir("sherpa/aar"))
        jniDir.set(layout.buildDirectory.dir("sherpa/jni"))
    }

val sherpaHost =
    tasks.register<FetchTask>("sherpaHost") {
        description = "sherpa-onnx $sherpaVersion JNI for Linux x86_64: the real recognizer in the JVM tests."
        url.set("$sherpaRelease/sherpa-onnx-v$sherpaVersion-linux-x64-jni.tar.bz2")
        sha256.set(sherpaHostSha)
        cacheFile.set("$cacheDir/sherpa-onnx-v$sherpaVersion-linux-x64-jni.tar.bz2")
        kind.set("tar")
        outputDir.set(layout.buildDirectory.dir("sherpa/host"))
    }

android {
    namespace = "fr.skimap.voicecmd"
    compileSdk = 37

    defaultConfig {
        minSdk = 29
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    testOptions {
        unitTests {
            isIncludeAndroidResources = true
            isReturnDefaultValues = true
        }
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
    }
}

androidComponents {
    onVariants { variant ->
        variant.sources.jniLibs?.addGeneratedSourceDirectory(sherpaAar, FetchTask::jniDir)
    }
}

tasks.withType<Test>().configureEach {
    val host = sherpaHost.flatMap { it.outputDir }
    inputs.dir(host).withPathSensitivity(PathSensitivity.NONE)
    val lib = host.map { it.asFile.resolve("lib").absolutePath }
    jvmArgumentProviders.add(CommandLineArgumentProvider { listOf("-Djava.library.path=${lib.get()}") })
    doFirst { environment("LD_LIBRARY_PATH", lib.get()) }
    // The end-to-end measurement (VoiceCommandAudioTest): its switches, from the command line.
    for (k in listOf("audio", "dir", "snr", "lang", "config")) {
        System.getProperty("skimap.voicecmd.$k")?.let { systemProperty("skimap.voicecmd.$k", it) }
    }
}

detekt {
    buildUponDefaultConfig = true
    allRules = false
    config.setFrom(files("$rootDir/config/detekt/detekt.yml"))
    source.setFrom(files("src/main/kotlin", "src/test/kotlin"))
}

ktlint {
    version.set(libs.versions.ktlint)
    android.set(true)
    filter {
        exclude { it.file.path.contains("/build/") }
    }
}

dependencies {
    api(files(sherpaAar.map { it.outputDir.file("classes.jar") }))
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.org.json)
    testImplementation(libs.robolectric)
    testImplementation(libs.androidx.test.core)

    detektPlugins(libs.detekt.compose.rules)
}
