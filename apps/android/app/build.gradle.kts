plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val repositoryRoot = rootDir.resolve("../..").canonicalFile
val corePackage = rootProject.layout.buildDirectory.dir("mav-core")
val releaseKeystorePath = providers.environmentVariable("ANDROID_KEYSTORE_PATH")
val releaseKeystorePassword = providers.environmentVariable("ANDROID_KEYSTORE_PASSWORD")
val releaseKeyAlias = providers.environmentVariable("ANDROID_KEY_ALIAS")
val releaseKeyPassword = providers.environmentVariable("ANDROID_KEY_PASSWORD")
val releaseSigningConfigured = listOf(
    releaseKeystorePath,
    releaseKeystorePassword,
    releaseKeyAlias,
    releaseKeyPassword,
).all { it.isPresent }

val buildMavCore by tasks.registering(Exec::class) {
    workingDir(repositoryRoot)
    commandLine("bash", "tools/platform/build_android.sh")
    inputs.files(
        fileTree(repositoryRoot.resolve("core")) {
            include(
                "Cargo.toml",
                "Cargo.lock",
                "crates/**/*.rs",
                "crates/**/Cargo.toml",
                "crates/**/uniffi.toml",
            )
        },
        repositoryRoot.resolve("tools/platform/build_android.sh"),
        repositoryRoot.resolve("tools/platform/lib.sh"),
    )
    outputs.dir(corePackage)
}

android {
    namespace = "com.sennnen.mav"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.sennnen.mav"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        buildConfigField("boolean", "MAV_CONNECTOR_MANAGER_ENABLED", "true")
        buildConfigField("boolean", "MAV_ALLOW_REMOTE_CONNECTORS", "true")
        buildConfigField("boolean", "MAV_ALLOW_THIRD_PARTY_CONNECTORS", "false")
        buildConfigField("boolean", "MAV_ALLOW_DEVELOPMENT_CONNECTORS", "false")
        buildConfigField("boolean", "MAV_SHOW_SYNTHETIC_DATA", "false")
        buildConfigField("String", "MAV_CONNECTOR_REGISTRY_URL", "\"\"")
        buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ID", "\"\"")
        buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ROOT_KEY_ID", "\"\"")
        buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ROOT_PUBLIC_KEY_HEX", "\"\"")
        buildConfigField("String", "MAV_CONNECTOR_PUBLISHER_KEY_ID", "\"\"")
        buildConfigField("String", "MAV_CONNECTOR_PUBLISHER_PUBLIC_KEY_HEX", "\"\"")
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    signingConfigs {
        if (releaseSigningConfigured) {
            create("release") {
                storeFile = file(releaseKeystorePath.get())
                storePassword = releaseKeystorePassword.get()
                keyAlias = releaseKeyAlias.get()
                keyPassword = releaseKeyPassword.get()
            }
        }
    }

    buildTypes {
        debug {
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
            buildConfigField("boolean", "MAV_ALLOW_THIRD_PARTY_CONNECTORS", "true")
            buildConfigField("boolean", "MAV_ALLOW_DEVELOPMENT_CONNECTORS", "true")
            buildConfigField("boolean", "MAV_SHOW_SYNTHETIC_DATA", "true")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_REGISTRY_URL",
                "\"https://raw.githubusercontent.com/sennnen/maverick-connectors/main/registry/index-v1.json\"",
            )
            buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ID", "\"dev.maverick.connectors\"")
            buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ROOT_KEY_ID", "\"registry-root-v1\"")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_REGISTRY_ROOT_PUBLIC_KEY_HEX",
                "\"7be29b98ba0937eadfa1fc1b47931f05dfce49faeee896c0f40c24a78d649fe6\"",
            )
            buildConfigField(
                "String", "MAV_CONNECTOR_PUBLISHER_KEY_ID", "\"maverick-whoop-live-test\"")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_PUBLISHER_PUBLIC_KEY_HEX",
                // Committed TEST publisher key (Ed25519 seed [151;32]) shared by every surface and
                // reproduced by maverick-connectors/tools/testsign.py. It signs the dev-registry
                // artifacts and locally sideloaded rebuilds alike. DEVELOPMENT scope only; every
                // production policy refuses it. Not a distribution key.
                "\"e1b71abfd3232804261e423f36556f6b4185bed41fdfd00d769ce15a394f43ce\"",
            )
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            buildConfigField("boolean", "MAV_ALLOW_THIRD_PARTY_CONNECTORS", "false")
            if (releaseSigningConfigured) {
                signingConfig = signingConfigs.getByName("release")
            }
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
        create("field") {
            initWith(getByName("release"))
            applicationIdSuffix = ".field"
            versionNameSuffix = "-field"
            buildConfigField("boolean", "MAV_ALLOW_THIRD_PARTY_CONNECTORS", "true")
            buildConfigField("boolean", "MAV_ALLOW_DEVELOPMENT_CONNECTORS", "true")
            buildConfigField("boolean", "MAV_SHOW_SYNTHETIC_DATA", "false")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_REGISTRY_URL",
                "\"https://raw.githubusercontent.com/sennnen/maverick-connectors/main/registry/index-v1.json\"",
            )
            buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ID", "\"dev.maverick.connectors\"")
            buildConfigField("String", "MAV_CONNECTOR_REGISTRY_ROOT_KEY_ID", "\"registry-root-v1\"")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_REGISTRY_ROOT_PUBLIC_KEY_HEX",
                "\"7be29b98ba0937eadfa1fc1b47931f05dfce49faeee896c0f40c24a78d649fe6\"",
            )
            buildConfigField(
                "String", "MAV_CONNECTOR_PUBLISHER_KEY_ID", "\"maverick-whoop-live-test\"")
            buildConfigField(
                "String",
                "MAV_CONNECTOR_PUBLISHER_PUBLIC_KEY_HEX",
                "\"e1b71abfd3232804261e423f36556f6b4185bed41fdfd00d769ce15a394f43ce\"",
            )
            matchingFallbacks += listOf("release")
        }
        // The field build in every respect that decides behaviour — minified, no synthetic data,
        // same registry and trust keys — but signed with the local debug key and installed under
        // its own id. `field` needs the release keystore, so on a machine without those secrets it
        // could be built and never run; the shipping configuration therefore went untested on
        // hardware, which is how an ECG capture that could not calibrate on a real strap shipped.
        create("sideload") {
            initWith(getByName("field"))
            applicationIdSuffix = ".sideload"
            versionNameSuffix = "-sideload"
            signingConfig = signingConfigs.getByName("debug")
            // Debuggable so a hardware run can pull what it produced — the generated report, the
            // store — back off the phone. This variant is never distributed.
            isDebuggable = true
            matchingFallbacks += listOf("field", "release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    sourceSets {
        getByName("main") {
            java.srcDir(corePackage.map { it.dir("Sources") })
            jniLibs.srcDir(corePackage.map { it.dir("jniLibs") })
        }
        getByName("test") {
            resources.srcDir("src/main/assets")
        }
        getByName("androidTest") {
            assets.srcDir(repositoryRoot.resolve("fixtures/ecg/corpus"))
        }
    }

    androidResources {
        noCompress += "tflite"
    }

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildMavCore)
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.06.00")
    implementation(composeBom)
    androidTestImplementation(composeBom)

    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.2")
    implementation("androidx.navigation:navigation-compose:2.7.7")
    implementation("androidx.health.connect:connect-client:1.1.0-alpha07")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.2")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.material3:material3")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    implementation("net.java.dev.jna:jna:5.12.0@aar")
    implementation("org.tensorflow:tensorflow-lite:2.17.0")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.1")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
