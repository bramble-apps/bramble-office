plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

import java.io.File

// Signed with Bramble Office's Android key, so it installs over earlier Notes APKs (kept in
// ~/.config/bramble, never in the repository; ANDROID_KEYSTORE / ANDROID_KEYSTORE_PASS override)
val ks = File(System.getenv("ANDROID_KEYSTORE") ?: "${System.getProperty("user.home")}/.config/bramble/android-release.jks")
val ksPass = System.getenv("ANDROID_KEYSTORE_PASS") ?: File(ks.path.removeSuffix(".jks") + ".pass").takeIf { it.exists() }?.readText()?.trim()

android {
    namespace = "app.bramble.notes"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "app.bramble.notes"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        if (ks.exists() && ksPass != null) {
            create("bramble") {
                storeFile = ks
                storePassword = ksPass
                keyAlias = "bramble"
                keyPassword = ksPass
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("bramble") ?: signingConfigs.getByName("debug")
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
