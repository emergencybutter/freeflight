import org.gradle.api.tasks.Exec

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

// The Cargo workspace this app's native core comes from: apps/android/ ->
// apps/ -> the repo root.
val repoRoot: File = rootProject.projectDir.parentFile.parentFile
val abis: List<String> = (project.findProperty("ff.abis") as String)
    .split(",").map(String::trim).filter(String::isNotEmpty)
val defaultApiBaseUrl = project.findProperty("ff.defaultApiBaseUrl") as String

val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
val uniffiBindings = layout.buildDirectory.dir("generated/uniffi")

/**
 * Cross-compiles `ff-uniffi` for each ABI and drops the `.so` files into a
 * jniLibs-shaped directory.
 *
 * Always release, even for a debug APK: this is a dependency being consumed,
 * not the code under debug, and a debug build of `rusqlite`'s bundled SQLite
 * is slow enough to be felt when panning the map.
 */
val cargoBuild by tasks.registering(Exec::class) {
    group = "build"
    description = "Cross-compiles the ff-uniffi Rust core for ${abis.joinToString()}"
    workingDir = repoRoot
    // `--package`, not `-p`: cargo-ndk claims `-p` for `--platform`.
    commandLine(
        buildList {
            add("cargo")
            add("ndk")
            abis.forEach { add("-t"); add(it) }
            add("-o"); add(rustJniLibs.get().asFile.absolutePath)
            add("build")
            add("--release")
            add("--package"); add("ff-uniffi")
        }
    )
    environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)

    // Re-runs when the core changes, and not otherwise — this is minutes of
    // work, so an incremental Compose edit must not trigger it.
    inputs.files(fileTree(repoRoot.resolve("crates")) { include("**/*.rs", "**/*.toml", "**/*.sql") })
    inputs.file(repoRoot.resolve("Cargo.toml"))
    inputs.file(repoRoot.resolve("Cargo.lock"))
    inputs.property("abis", abis)
    outputs.dir(rustJniLibs)
}

/**
 * Generates the Kotlin bindings from the library that was just built, so
 * the bindings can never describe a different core than the `.so` shipped
 * next to them.
 */
val generateUniffiBindings by tasks.registering(Exec::class) {
    group = "build"
    description = "Generates Kotlin bindings for the ff-uniffi core"
    dependsOn(cargoBuild)
    workingDir = repoRoot
    doFirst {
        val builtLibrary = abis.asSequence()
            .map { rustJniLibs.get().asFile.resolve("$it/libff_uniffi.so") }
            .firstOrNull(File::exists)
            ?: error("cargoBuild produced no libff_uniffi.so for any of: $abis")
        commandLine(
            "cargo", "run", "--package", "ff-uniffi", "--features", "cli",
            "--bin", "uniffi-bindgen", "--",
            "generate", "--library", builtLibrary.absolutePath,
            "--language", "kotlin",
            "--out-dir", uniffiBindings.get().asFile.absolutePath,
        )
    }
    inputs.dir(rustJniLibs)
    outputs.dir(uniffiBindings)
}

android {
    namespace = "ws.freeflight"
    compileSdk = 35
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "ws.freeflight"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        buildConfigField("String", "DEFAULT_API_BASE_URL", "\"$defaultApiBaseUrl\"")
        ndk { abiFilters.addAll(abis) }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].kotlin.srcDir(uniffiBindings)
    sourceSets["main"].jniLibs.srcDir(rustJniLibs)

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

// Kotlin compilation needs the generated bindings to exist first; the
// jniLibs merge needs the `.so` files.
tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    dependsOn(generateUniffiBindings)
}
tasks.matching { it.name.startsWith("merge") && it.name.contains("JniLibFolders") }
    .configureEach { dependsOn(cargoBuild) }

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons)
    implementation(libs.maplibre)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.kotlinx.coroutines.android)

    // UniFFI's generated Kotlin binds the `.so` through JNA, and needs the
    // Android `@aar` artifact (the plain jar has no native dispatch for
    // Android ABIs). Spelled out rather than `libs.jna` so the classifier
    // is visible; the version still comes from the catalog.
    val jna = libs.jna.get()
    implementation("${jna.module.group}:${jna.module.name}:${jna.versionConstraint.requiredVersion}@aar")

    debugImplementation(libs.androidx.compose.ui.tooling)

    testImplementation(libs.junit)
    androidTestImplementation(libs.androidx.junit)
    androidTestImplementation(libs.androidx.espresso.core)
    androidTestImplementation(platform(libs.androidx.compose.bom))
}
