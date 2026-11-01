package app.bramble.keydeck

import android.content.Intent
import android.net.Uri
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import io.flutter.view.TextureRegistry

class MainActivity : FlutterActivity() {
    // the slideshow's movies and sounds (kn-app showapi "movies"): Android's MediaPlayer, the
    // picture on a Flutter texture; id -> (player, texture)
    private val players = HashMap<Long, Pair<android.media.MediaPlayer, TextureRegistry.SurfaceTextureEntry?>>()

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
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.bramble.keydeck/video").setMethodCallHandler { call, result ->
            when (call.method) {
                // {path, start, end, volume, loop, video}: the texture's id (or -1 for a sound)
                "play" -> {
                    try {
                        val path = call.argument<String>("path")!!
                        val start = (call.argument<Double>("start") ?: 0.0)
                        val end = (call.argument<Double>("end") ?: 0.0)
                        val video = call.argument<Boolean>("video") ?: true
                        val entry = if (video) flutterEngine.renderer.createSurfaceTexture() else null
                        val mp = android.media.MediaPlayer()
                        mp.setDataSource(path)
                        if (entry != null) mp.setSurface(android.view.Surface(entry.surfaceTexture()))
                        val vol = (call.argument<Double>("volume") ?: 1.0).toFloat()
                        mp.setVolume(vol, vol)
                        mp.isLooping = (call.argument<Boolean>("loop") ?: false) && end <= 0.0
                        mp.setOnPreparedListener {
                            if (start > 0) it.seekTo((start * 1000).toInt())
                            it.start()
                        }
                        // a trimmed end: stop (or loop back) there
                        if (end > 0.0) {
                            val h = android.os.Handler(mainLooper)
                            val loop = call.argument<Boolean>("loop") ?: false
                            val tick = object : Runnable {
                                override fun run() {
                                    try {
                                        if (!mp.isPlaying) return
                                        if (mp.currentPosition >= end * 1000) {
                                            if (loop) mp.seekTo((start * 1000).toInt()) else { mp.pause(); return }
                                        }
                                        h.postDelayed(this, 50)
                                    } catch (e: Exception) {}
                                }
                            }
                            mp.setOnInfoListener { _, what, _ -> if (what == android.media.MediaPlayer.MEDIA_INFO_VIDEO_RENDERING_START) h.post(tick); false }
                            h.postDelayed(tick, 300)
                        }
                        mp.prepareAsync()
                        val id = entry?.id() ?: (-1000L - players.size)
                        players[id] = Pair(mp, entry)
                        mp.setOnVideoSizeChangedListener { _, w, hgt -> if (w > 0 && hgt > 0) entry?.surfaceTexture()?.setDefaultBufferSize(w, hgt) }
                        result.success(id)
                    } catch (e: Exception) {
                        result.success(null)
                    }
                }
                "stop" -> {
                    val id = (call.arguments as Number).toLong()
                    players.remove(id)?.let { (mp, entry) ->
                        try { mp.stop() } catch (e: Exception) {}
                        mp.release()
                        entry?.release()
                    }
                    result.success(true)
                }
                else -> result.notImplemented()
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
