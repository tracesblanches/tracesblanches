// Network module of the app (K2 #52): the only code that opens a connection. It fetches
// the server catalog and site indexes (ETag, 304) and downloads pack bundles with resume
// (Range / If-Range). Deciding what to download and installing it is the Rust core's job
// (crates/ffi, module sync), called by :app.
plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

android {
    namespace = "fr.skimap.sync"
    compileSdk = 37

    defaultConfig {
        minSdk = 29
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
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
}

dependencies {
    implementation(libs.kotlinx.coroutines.android)
    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    detektPlugins(libs.detekt.compose.rules)
}
