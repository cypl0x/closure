plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "net.wolfhard.closure"
    compileSdk = flutter.compileSdkVersion
    // The NDK Flutter asks for, supplied by the nix SDK build rather
    // than downloaded — the store is read-only, so gradle cannot fetch
    // one and fails with "SDK directory is not writable" instead.
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "net.wolfhard.closure"

        // arm64 only, and deliberately. Exactly one Rust target is
        // cross-compiled into jniLibs, so an APK carrying armeabi-v7a or
        // x86_64 would install happily on those devices and then crash
        // at the first FFI call — a working install is a worse failure
        // than no install, because it looks like the app is broken
        // rather than unsupported.
        ndk {
            abiFilters.clear()
            abiFilters += listOf("arm64-v8a")
        }
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    buildTypes {
        release {
            // TODO: Add your own signing config for the release build.
            // Signing with the debug keys for now, so `flutter run --release` works.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
