plugins { id("com.android.application") }

android {
    namespace = "app.fta.android"
    compileSdk = 37
    defaultConfig {
        applicationId = "app.fta.android"
        minSdk = 29
        targetSdk = 37
        versionCode = 5
        versionName = "0.1.0"
        testInstrumentationRunner = "app.fta.android.FtaChecks"
        ndk { abiFilters += "arm64-v8a" }
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    sourceSets.getByName("main").jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs").get().asFile)
    buildTypes { release { isMinifyEnabled = false } }
}

// Rust streams file bodies; JNI transports bounded JSON control messages and native key-vault calls only.
val buildRust by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir)
    commandLine("bash", "build-native.sh")
    inputs.file(rootProject.file("build-native.sh"))
    inputs.file(rootProject.file("../../Cargo.toml"))
    inputs.file(rootProject.file("../../Cargo.lock"))
    inputs.dir(rootProject.file("bridge"))
    inputs.dir(rootProject.file("native"))
    inputs.dir(rootProject.file("../../crates/fta-core"))
    inputs.dir(rootProject.file("../../crates/fta-platform"))
    inputs.dir(rootProject.file("../ios/bridge"))
    outputs.file(layout.buildDirectory.file("generated/jniLibs/arm64-v8a/libfta_android.so"))
}
tasks.named("preBuild").configure { dependsOn(buildRust) }

// Fail closed on stale notices: dependency changes require reviewing the exact Android target inventory.
val checkNotices by tasks.registering(Exec::class) {
    // The native build fetches the locked target sources before the offline license inventory reads them.
    dependsOn(buildRust)
    workingDir(rootProject.projectDir)
    commandLine("python3", "scripts/generate-notices.py", "--check")
    environment("PATH", System.getenv("HOME") + "/.cargo/bin:" + System.getenv("PATH"))
}
tasks.named("preBuild").configure { dependsOn(checkNotices) }
