pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "skimap"
include(":app", ":benchmark")
// Pack download (K2): the network module of the app.
include(":sync")
// The outing (K4, K5, K13): the location service, the voice, the buttons.
include(":tracking")
// Sending the outing (K11): Health Connect and Strava.
include(":sport")
// Collaborative and sport (V8 to V15): reports, offline queue, OSM notes, « Mes sorties ».
include(":community")
