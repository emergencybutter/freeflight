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

// 16 KB page size support for the Rust core. The NDK already passes
// `max-page-size`, which is what aligns the LOAD segments; `common-page-size`
// is the separate one that pads the GNU_RELRO region, and without it RELRO
// ends mid-page (measured: 0x2e5000 before, 0x2e8000 after). A device with
// 16 KB pages reports that as "RELRO segment not aligned" — see the
// alignment check below for the incident that turned this up.
val sixteenKbRustFlags =
    "-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384"

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
    environment("RUSTFLAGS", sixteenKbRustFlags)

    // Re-runs when the core changes, and not otherwise — this is minutes of
    // work, so an incremental Compose edit must not trigger it.
    inputs.files(fileTree(repoRoot.resolve("crates")) { include("**/*.rs", "**/*.toml", "**/*.sql") })
    inputs.file(repoRoot.resolve("Cargo.toml"))
    inputs.file(repoRoot.resolve("Cargo.lock"))
    inputs.property("abis", abis)
    inputs.property("rustflags", sixteenKbRustFlags)
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

/**
 * Checks every packaged 64-bit `.so` for 16 KB page-size compatibility.
 *
 * A device with 16 KB memory pages refuses to load a library laid out for
 * 4 KB pages, and nothing in a normal build tells you: it installs fine,
 * and only that hardware complains. This app packages native code from
 * three independent sources — the Rust core, MapLibre's AAR and JNA's — so
 * any of them can reintroduce the problem through a routine bump. JNA did:
 * 5.14's `libjnidispatch.so` ended its RELRO region at 0x36000, and a
 * Pixel reported exactly "libjnidispatch.so : RELRO segment not aligned".
 * That is why the version catalog pins JNA far above uniffi's actual floor.
 *
 * Two different things get checked, and they fail differently:
 *
 *  - **LOAD segment alignment** must be at least 16 KB, for every packaged
 *    library whoever built it. This is the documented hard requirement.
 *  - **Where the GNU_RELRO region ends** must land on a 16 KB boundary —
 *    but only for libraries built from this repo, which in practice means
 *    `libff_uniffi.so`. Third-party prebuilts are deliberately exempt:
 *    `libandroidx.graphics.path.so` ends its RELRO mid-page in both 1.0.1
 *    and the current 1.1.0, shipped that way by Google, and a consumer can
 *    do nothing about it. Policing it would mean a warning on every single
 *    build that nobody can act on, which is how warnings get ignored.
 *
 * Only 64-bit ABIs are considered: 16 KB pages are a 64-bit concern, and
 * `armeabi-v7a`/`x86` always page at 4 KB.
 */
val sixtyFourBitAbis = setOf("arm64-v8a", "x86_64")

/** Built here, so their linker flags are ours to get right. */
val ourOwnLibraries = setOf("libff_uniffi.so")

fun verifySixteenKbAlignment(libs: Iterable<File>, readelf: File, logger: Logger) {
    fun headers(lib: File) = providers.exec {
        commandLine(readelf.absolutePath, "-l", lib.absolutePath)
    }.standardOutput.asText.get().lineSequence().map { it.trim().split(Regex("""\s+""")) }

    val unalignedLoad = mutableListOf<String>()
    val unalignedRelro = mutableListOf<String>()

    libs.filter { it.name.endsWith(".so") && it.parentFile.name in sixtyFourBitAbis }
        .sortedBy { it.path }
        .forEach { lib ->
            val name = "${lib.parentFile.name}/${lib.name}"
            val rows = headers(lib).toList()

            rows.filter { it.firstOrNull() == "LOAD" }
                .mapNotNull { it.lastOrNull()?.removePrefix("0x")?.toLongOrNull(16) }
                .minOrNull()
                ?.let { if (it < 16384) unalignedLoad += "$name (0x${it.toString(16)})" }

            // Columns: Type Offset VirtAddr PhysAddr FileSiz MemSiz Flg Align
            if (lib.name in ourOwnLibraries) {
                rows.firstOrNull { it.firstOrNull() == "GNU_RELRO" }?.let { row ->
                    val vaddr = row.getOrNull(2)?.removePrefix("0x")?.toLongOrNull(16)
                    val memsz = row.getOrNull(5)?.removePrefix("0x")?.toLongOrNull(16)
                    if (vaddr != null && memsz != null && (vaddr + memsz) % 16384L != 0L) {
                        unalignedRelro += "$name (RELRO ends at 0x${(vaddr + memsz).toString(16)})"
                    }
                }
            }
        }

    val failures = unalignedLoad.map { "$it - LOAD segment below 16 KB" } +
        unalignedRelro.map { "$it - RELRO must end on a 16 KB boundary" }
    if (failures.isNotEmpty()) {
        throw GradleException(
            buildString {
                appendLine("These native libraries are not 16 KB page size compatible:")
                failures.forEach { appendLine("  $it") }
                appendLine("For our own libraries, check the linker flags in cargoBuild")
                appendLine("(-Wl,-z,max-page-size=16384 aligns segments, and")
                appendLine("-Wl,-z,common-page-size=16384 pads RELRO).")
                append("For a dependency, upgrade it: JNA needed 5.15 or newer.")
            }
        )
    }
}

tasks.matching { it.name.matches(Regex("""merge[A-Z]\w*NativeLibs""")) }.configureEach {
    val readelf = fileTree("${android.ndkDirectory}/toolchains/llvm/prebuilt") {
        include("**/bin/llvm-readelf", "**/bin/llvm-readelf.exe")
    }.files.firstOrNull()
    val taskLogger = logger
    doLast {
        if (readelf == null) {
            taskLogger.warn("16 KB alignment check skipped: no llvm-readelf under ${android.ndkDirectory}")
            return@doLast
        }
        verifySixteenKbAlignment(outputs.files.asFileTree.files, readelf, taskLogger)
    }
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
