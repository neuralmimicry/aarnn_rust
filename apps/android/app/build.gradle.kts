plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val generatedJniLibs = layout.buildDirectory.dir("generated/jniLibs")
val withRust = providers.gradleProperty("withRust").map(String::toBoolean).orElse(false)

android {
    namespace = "com.neuralmimicry.aarnn"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.neuralmimicry.aarnn"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

// AGP 9 rejects Provider instances in the legacy SourceSet API. The task
// below still owns and declares this generated directory; only its concrete
// path is registered with the Android packaging model.
sourceSets["main"].jniLibs.directories.add(generatedJniLibs.get().asFile.absolutePath)
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.activity:activity-compose:1.8.2")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
}

val buildRust = tasks.register("buildRustAndroid") {
    description = "Builds the shared Rust cdylib into generated Android ABI output."
    group = "native"
    onlyIf { withRust.get() }
    outputs.dir(generatedJniLibs)

    doLast {
        val targets = mapOf(
            "x86_64" to "x86_64-linux-android",
            "arm64-v8a" to "aarch64-linux-android",
        )
        targets.forEach { (abi, target) ->
            val output = generatedJniLibs.get().asFile.resolve(abi)
            val process = ProcessBuilder(
                "cargo", "xtask", "build", "--product", "android",
                "--target", target, "--abi", abi, "--out-dir", output.absolutePath,
            )
                .directory(rootProject.projectDir.parentFile.parentFile)
                .inheritIO()
                .start()
            check(process.waitFor() == 0) { "cargo xtask Android build failed for $abi" }
        }
    }
}

tasks.named("preBuild") { dependsOn(buildRust) }
