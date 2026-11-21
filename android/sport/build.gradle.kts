// Sharing a finished outing (#63): the GPX and FIT files the core exported, given to the
// share menu (ACTION_SEND, FileProvider), and the session with its route, distance and
// climb written to Health Connect when the user allows it at the share. No account, no
// key, no network. The app gives the outing (fr.skimap.sport.SharedOuting, from the core)
// and hosts the Health Connect rationale (HealthRationaleActivity, subclassed for its theme).
plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.roborazzi)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

android {
    namespace = "fr.skimap.sport"
    compileSdk = 37

    defaultConfig {
        minSdk = 29
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    // The outing of the JVM and of the instrumented tests.
    sourceSets {
        getByName("test").kotlin.srcDir("src/sharedTest/kotlin")
        getByName("androidTest").kotlin.srcDir("src/sharedTest/kotlin")
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

detekt {
    buildUponDefaultConfig = true
    allRules = false
    config.setFrom(files("$rootDir/config/detekt/detekt.yml"))
    source.setFrom(files("src/main/kotlin", "src/test/kotlin", "src/sharedTest/kotlin", "src/androidTest/kotlin"))
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
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.androidx.health.connect.client)
    debugImplementation(libs.compose.ui.tooling)
    debugImplementation(libs.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.robolectric)
    testImplementation(libs.roborazzi)
    testImplementation(libs.roborazzi.compose)
    testImplementation(libs.roborazzi.junit.rule)
    testImplementation(libs.androidx.test.core)
    testImplementation(platform(libs.compose.bom))
    testImplementation(libs.compose.ui.test.junit4)

    androidTestImplementation(libs.junit)
    androidTestImplementation(libs.androidx.test.core)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.kotlinx.coroutines.test)

    detektPlugins(libs.detekt.compose.rules)
}
