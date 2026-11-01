plugins { id("com.android.application") }

android {
    namespace = "app.fta.android"
    compileSdk = 37
    defaultConfig {
        applicationId = "app.fta.android"
        minSdk = 29
        targetSdk = 37
        // Identify the localized Play release candidate without changing existing app identity.
        versionCode = 7
        versionName = "0.1.0"
        testInstrumentationRunner = "app.fta.android.FtaChecks"
        ndk { abiFilters += "arm64-v8a" }
    }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_17; targetCompatibility = JavaVersion.VERSION_17 }
    buildFeatures { buildConfig = true }
    // Local release instrumentation uses the same upload candidate only in an isolated emulator; test code never enters the AAB.
    testBuildType = providers.gradleProperty("ftaTestBuildType").getOrElse("debug")
    // Debug and release must never package one another’s native binary or signing identity.
    sourceSets.getByName("debug").jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs/debug").get().asFile)
    sourceSets.getByName("release").jniLibs.srcDir(layout.buildDirectory.dir("generated/jniLibs/release").get().asFile)
    // Upload credentials are supplied by the local Keychain-backed wrapper, never stored in Gradle files/cache.
    val uploadPath = System.getenv("FTA_ANDROID_UPLOAD_KEYSTORE")
    val upload = if (uploadPath.isNullOrBlank()) null else signingConfigs.create("upload") {
        storeFile = file(uploadPath)
        storePassword = System.getenv("FTA_ANDROID_UPLOAD_STORE_PASSWORD") ?: error("Use scripts/build-release.py for signing")
        keyAlias = "fta-upload"
        keyPassword = storePassword
    }
    buildTypes { release { isMinifyEnabled = false; isDebuggable = false; signingConfig = upload } }
    // All small catalogs remain available offline when Android changes the app language.
    bundle { language { enableSplit = false } }
}

// Native profiles are variant-specific: Play packages optimized Rust without adding another engine implementation.
for (profile in listOf("debug", "release")) {
    val nativeBuild = tasks.register<Exec>("buildRust" + profile.replaceFirstChar { it.uppercaseChar() }) {
        workingDir(rootProject.projectDir)
        commandLine("bash", "build-native.sh", profile)
        inputs.file(rootProject.file("build-native.sh"))
        inputs.file(rootProject.file("../../Cargo.toml"))
        inputs.file(rootProject.file("../../Cargo.lock"))
        for (source in listOf("bridge", "native", "../../crates/fta-core", "../../crates/fta-platform", "../ios/bridge")) inputs.dir(rootProject.file(source))
        outputs.file(layout.buildDirectory.file("generated/jniLibs/$profile/arm64-v8a/libfta_android.so"))
    }
    // Native compilation fetches the locked target sources before offline notice inventory, including on a fresh checkout.
    val notices = tasks.register<Exec>("checkNotices" + profile.replaceFirstChar { it.uppercaseChar() }) {
        dependsOn(nativeBuild)
        workingDir(rootProject.projectDir)
        commandLine("python3", "scripts/generate-notices.py", "--check")
        environment("PATH", System.getenv("HOME") + "/.cargo/bin:" + System.getenv("PATH"))
    }
    tasks.configureEach { if (name == "pre" + profile.replaceFirstChar { it.uppercaseChar() } + "Build") dependsOn(notices) }
}
// Keep the existing explicit notice-check entry point without compiling the unused release variant.
tasks.register("checkNotices") { dependsOn("checkNoticesDebug") }
val checkLocalizations by tasks.registering(Exec::class) {
    workingDir(rootProject.projectDir)
    commandLine("python3", "scripts/generate-localizations.py", "--check")
}
tasks.named("preBuild").configure { dependsOn(checkLocalizations) }
