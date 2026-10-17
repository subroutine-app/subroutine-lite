
import java.net.URI


plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

val liteApplicationId = "com.example.subroutine_lite"


fun booleanProperty(name: String): Boolean = providers.gradleProperty(name).orNull?.let {
    requireNotNull(it.toBooleanStrictOrNull()) { "$name must be true or false" }
} ?: false

val offlineOnly = booleanProperty("subroutineLite.offlineOnly")
val rauthyLoopback = booleanProperty("rauthyAllowInsecureLoopback")
val endpointDefaults = mapOf(
    "subroutineLite.apiBaseUrl" to "https://lite-api.subroutineapp.com/",
    "rauthyIssuer" to "https://auth.subroutineapp.com/auth/v1/",
    "rauthyClientId" to "subroutine-android",
)
val endpointOverrides = endpointDefaults.mapValues { (name, _) -> providers.gradleProperty(name).orNull }
require(
    (endpointOverrides.values.all { it == null } && !rauthyLoopback) ||
        endpointOverrides.values.all { it != null },
) {
    "Endpoint overrides require the full tuple: subroutineLite.apiBaseUrl, rauthyIssuer, rauthyClientId. " +
        "Omit all three for Fermi; use subroutineLite.offlineOnly=true for local-only use."
}
val endpoints = endpointDefaults.mapValues { (name, default) -> endpointOverrides[name] ?: default }
val apiBaseUrl = endpoints.getValue("subroutineLite.apiBaseUrl")
val rauthyIssuer = endpoints.getValue("rauthyIssuer")
val rauthyClientId = endpoints.getValue("rauthyClientId")
require(rauthyClientId.isNotEmpty() && rauthyClientId.all { it in '!'..'~' }) { "Invalid rauthyClientId" }
for ((name, value) in mapOf("subroutineLite.apiBaseUrl" to apiBaseUrl, "rauthyIssuer" to rauthyIssuer)) {
    val uri = runCatching { URI(value) }.getOrNull()
    require(uri != null && uri.isAbsolute && !uri.host.isNullOrEmpty() &&
        uri.rawUserInfo == null && uri.rawQuery == null && uri.rawFragment == null &&
        (uri.port == -1 || uri.port in 1..65535) && !uri.rawAuthority.endsWith(":") &&
        (uri.scheme == "https" || (rauthyLoopback && uri.scheme == "http" &&
            uri.host in setOf("localhost", "127.0.0.1", "[::1]")))) {
        "$name must be an absolute HTTPS URL without user info, query or fragment and with a valid port; " +
            "HTTP needs explicit debug literal-loopback opt-in (not 10.0.2.2)."
    }
}
require(apiBaseUrl.endsWith("/")) { "subroutineLite.apiBaseUrl must end with /" }
val usesHttp = URI(apiBaseUrl).scheme == "http" || URI(rauthyIssuer).scheme == "http"
val validateReleaseConfiguration = tasks.register("validateReleaseConfiguration") {
    doLast {
        require(!usesHttp) { "Release requires HTTPS for both API and issuer, even in offline-only mode." }
    }
}
tasks.matching { it.name in setOf("preReleaseBuild", "generateReleaseBuildConfig") }.configureEach {
    dependsOn(validateReleaseConfiguration)
}

fun buildConfigString(value: String): String = "\"" + value
    .replace("\\", "\\\\").replace("\"", "\\\"")
    .replace("\n", "\\n").replace("\r", "\\r") + "\""

android {
    namespace = "com.example.subroutine_simple"
    ndkVersion = "29.0.14206865"
    compileSdk {
        version = release(36)
    }

    defaultConfig {
        applicationId = liteApplicationId
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"


        buildConfigField("boolean", "OFFLINE_ONLY", offlineOnly.toString())
        buildConfigField("String", "API_BASE_URL", buildConfigString(apiBaseUrl))
        buildConfigField("String", "RAUTHY_ISSUER", buildConfigString(rauthyIssuer))
        buildConfigField("String", "RAUTHY_CLIENT_ID", buildConfigString(rauthyClientId))
        manifestPlaceholders["appAuthRedirectScheme"] = "dev.subroutine.lite"
        manifestPlaceholders["remoteEnabled"] = (!offlineOnly).toString()


    }

    buildTypes {
        debug {
            buildConfigField("boolean", "RAUTHY_ALLOW_INSECURE_LOOPBACK", rauthyLoopback.toString())
        }
        release {
            buildConfigField("boolean", "RAUTHY_ALLOW_INSECURE_LOOPBACK", "false")
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }
    if (offlineOnly) {
        sourceSets.getByName("debug").manifest.srcFile("src/offline/AndroidManifest.xml")
        sourceSets.getByName("release").manifest.srcFile("src/offline/AndroidManifest.xml")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_11)
            freeCompilerArgs.addAll(
                "-opt-in=androidx.compose.material3.ExperimentalMaterial3ExpressiveApi",
                "-opt-in=androidx.compose.material3.ExperimentalMaterial3Api",
            )
        }
    }
    buildFeatures {
        buildConfig = true
        compose = true
    }
}

dependencies {
    implementation(libs.appauth)

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.runtime.ktx)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.ui.graphics)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.material.icons.extended)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.retrofit)
    implementation(libs.retrofit.kotlinx.serialization.converter)
    implementation(libs.okhttp)
    implementation(libs.okhttp.logging.interceptor)
    implementation(libs.okhttp.sse)

    implementation(libs.androidx.navigation3.runtime)
    implementation(libs.androidx.navigation3.ui)
    implementation(libs.androidx.lifecycle.viewmodel.navigation3)
    implementation(libs.androidx.work.runtime.ktx)

    debugImplementation(libs.androidx.compose.ui.tooling)
    testImplementation(libs.junit)
    testImplementation(libs.robolectric)

}

val nativeParser = rootProject.layout.projectDirectory.dir("native")
val nativeParserInputs = files(
    fileTree(nativeParser) { include("src/**", "Cargo.toml", "Cargo.lock", "build.nu") },
    fileTree(rootProject.file("../crates/parser")) { include("src/**", "Cargo.toml") },
    fileTree(rootProject.file("../crates/core")) { include("src/**", "Cargo.toml") },
    rootProject.file("../Cargo.toml"),
)
val parserJniOutput = nativeParser.dir("target/jniLibs/release")
val buildParserJni = tasks.register<Exec>("buildParserJni") {
    group = "build"
    description = "Build the shared Rust parser for Android (requires Rust, cargo-ndk and Nushell)."
    inputs.files(nativeParserInputs)
    inputs.property("ndkVersion", android.ndkVersion)
    outputs.dir(parserJniOutput)
    workingDir(rootProject.projectDir)
    if (gradle.startParameter.isOffline) environment("CARGO_NET_OFFLINE", "true")
    commandLine(
        "nu", nativeParser.file("build.nu").asFile.absolutePath,
        "--sdk", androidComponents.sdkComponents.sdkDirectory.get().asFile.absolutePath,
        "--api-level", "26",
    )
}
android.sourceSets.getByName("main").jniLibs.srcDir(parserJniOutput)
tasks.named("preBuild").configure { dependsOn(buildParserJni) }

val parserHostOutput = nativeParser.dir("target/debug")
val buildParserHost = tasks.register<Exec>("buildParserHost") {
    group = "verification"
    description = "Build the real host JNI parser for JVM integration tests."
    inputs.files(nativeParserInputs)
    outputs.file(parserHostOutput.file(System.mapLibraryName("subroutine_parser_jni")))
    workingDir(nativeParser)
    if (gradle.startParameter.isOffline) environment("CARGO_NET_OFFLINE", "true")
    commandLine("cargo", "build", "--locked", "--target-dir", nativeParser.dir("target").asFile.absolutePath)
}
tasks.withType<Test>().configureEach {
    dependsOn(buildParserHost)
    systemProperty("java.library.path", parserHostOutput.asFile.absolutePath)
}
