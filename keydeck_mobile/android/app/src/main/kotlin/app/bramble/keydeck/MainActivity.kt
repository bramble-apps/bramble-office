package app.bramble.keydeck

import android.content.Intent
import android.net.Uri
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    // web pages the app opens (Support Development…): the system's browser
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.bramble.keydeck/open").setMethodCallHandler { call, result ->
            if (call.method == "url") {
                try {
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(call.arguments as String)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                    result.success(true)
                } catch (e: Exception) {
                    result.success(false)
                }
            } else {
                result.notImplemented()
            }
        }
        // the device's name, as Keydeck Remote shows it on the computer
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.bramble.keydeck/device").setMethodCallHandler { call, result ->
            if (call.method == "name") {
                val n = android.provider.Settings.Global.getString(contentResolver, "device_name")
                result.success(if (n.isNullOrBlank()) "${android.os.Build.MANUFACTURER} ${android.os.Build.MODEL}" else n)
            } else {
                result.notImplemented()
            }
        }
    }
}
