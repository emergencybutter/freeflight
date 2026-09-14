pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
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

// Deliberately not part of the Cargo workspace's Gradle-less world: the
// Rust crates are built from here by the `:app` module's cargo tasks, but
// this build is rooted at apps/android so `gradlew` never walks the repo.
rootProject.name = "freeflight"
include(":app")
