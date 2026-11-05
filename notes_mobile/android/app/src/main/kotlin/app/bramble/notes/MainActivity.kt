package app.bramble.notes

import android.content.ContentValues
import android.content.Intent
import android.media.MediaPlayer
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.view.KeyEvent
import android.view.KeyboardShortcutGroup
import android.view.KeyboardShortcutInfo
import android.view.Menu
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        highRefresh()
    }

    /** The display's fastest mode at its current size (120 Hz where it has it): Android gives
     *  apps 60 Hz on many phones unless they ask. */
    private fun highRefresh() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
        val d = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) display else @Suppress("DEPRECATION") windowManager.defaultDisplay
        d ?: return
        val now = d.mode
        val best = d.supportedModes.filter { it.physicalWidth == now.physicalWidth && it.physicalHeight == now.physicalHeight }
            .maxByOrNull { it.refreshRate } ?: return
        window.attributes = window.attributes.also { it.preferredDisplayModeId = best.modeId }
    }

    private var player: MediaPlayer? = null
    private var photo: Uri? = null
    private var photoResult: MethodChannel.Result? = null
    private var photoKeep = true
    private var channel: MethodChannel? = null
    private var tree: Pair<File, String>? = null
    private var treeResult: MethodChannel.Result? = null
    private var saving: File? = null

    /** Android 13's notification permission asked for already (this run). */
    private var asked = false
    private var savingResult: MethodChannel.Result? = null

    /// What the app was opened for (Quick Note, New Note, something shared to it), until Dart asks.
    private var pending: Map<String, Any?>? = null

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        val a = parse(intent) ?: return
        channel?.invokeMethod("intent", a)
    }

    private fun parse(i: Intent?): Map<String, Any?>? {
        i ?: return null
        return when (i.action) {
            "app.bramble.notes.QUICK_NOTE" -> mapOf("kind" to "quicknote")
            "app.bramble.notes.NEW_NOTE" -> mapOf("kind" to "newnote")
            Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> {
                val uris = mutableListOf<Uri>()
                if (i.action == Intent.ACTION_SEND) {
                    @Suppress("DEPRECATION")
                    (i.getParcelableExtra<Uri>(Intent.EXTRA_STREAM))?.let { uris.add(it) }
                } else {
                    @Suppress("DEPRECATION")
                    i.getParcelableArrayListExtra<Uri>(Intent.EXTRA_STREAM)?.let { uris.addAll(it) }
                }
                mapOf(
                    "kind" to "share",
                    "subject" to i.getStringExtra(Intent.EXTRA_SUBJECT),
                    "text" to i.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString(),
                    "files" to uris.mapNotNull { copyIn(it) },
                )
            }
            else -> null
        }
    }

    /// A shared file copied into the app's cache under its own name.
    private fun copyIn(uri: Uri): String? = try {
        var name = "Shared ${System.currentTimeMillis()}"
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0)?.let { name = it }
        }
        val dir = File(cacheDir, "shared-${System.nanoTime()}").apply { mkdirs() }
        val out = File(dir, name.replace('/', '_'))
        contentResolver.openInputStream(uri)?.use { i -> out.outputStream().use { o -> i.copyTo(o) } }
        out.path
    } catch (e: Exception) {
        null
    }

    /// Slow work (scans, PDF pages) off the main thread, answered on it.
    private fun background(result: MethodChannel.Result, work: () -> Any?) {
        Thread {
            val r = try { work() } catch (e: Exception) { null }
            runOnUiThread { result.success(r) }
        }.start()
    }

    // the system's share sheet (Send a Copy), web links in notes, the camera (Take Photo), and
    // playing an audio attachment
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        pending = parse(intent)
        channel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.bramble.notes/system")
        channel!!.setMethodCallHandler { call, result ->
            try {
                when (call.method) {
                    "pending" -> {
                        result.success(pending)
                        pending = null
                    }
                    "detectEdges" -> background(result) { Scan.detect(call.arguments as String) }
                    "warp" -> {
                        val a = call.arguments as Map<*, *>
                        @Suppress("UNCHECKED_CAST")
                        background(result) { Scan.warp(a["path"] as String, (a["corners"] as List<Number>).map { it.toDouble() }, a["filter"] as String, a["out"] as String) }
                    }
                    "makePdf" -> {
                        val a = call.arguments as Map<*, *>
                        @Suppress("UNCHECKED_CAST")
                        background(result) { Scan.pdf(a["pages"] as List<String>, a["out"] as String) }
                    }
                    "pdfInfo" -> background(result) { Scan.pdfInfo(call.arguments as String) }
                    "pdfRender" -> {
                        val a = call.arguments as Map<*, *>
                        background(result) { Scan.pdfRender(a["path"] as String, (a["page"] as Number).toInt(), (a["width"] as Number).toInt(), a["out"] as String) }
                    }
                    "url" -> {
                        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(call.arguments as String)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                        result.success(true)
                    }
                    "shareText" -> {
                        val a = call.arguments as Map<*, *>
                        val send = Intent(Intent.ACTION_SEND).setType("text/plain")
                            .putExtra(Intent.EXTRA_SUBJECT, a["title"] as String? ?: "")
                            .putExtra(Intent.EXTRA_TEXT, a["text"] as String? ?: "")
                        startActivity(Intent.createChooser(send, null))
                        result.success(true)
                    }
                    "takePhoto" -> {
                        // the picture goes to the gallery (a MediaStore row of our own) and is copied for the note
                        val values = ContentValues().apply {
                            put(MediaStore.Images.Media.DISPLAY_NAME, "Notes ${System.currentTimeMillis()}.jpg")
                            put(MediaStore.Images.Media.MIME_TYPE, "image/jpeg")
                        }
                        val uri = contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values)
                        if (uri == null) {
                            result.success(null)
                        } else {
                            photo = uri
                            photoResult = result
                            photoKeep = (call.arguments as? Map<*, *>)?.get("save") as? Boolean ?: true
                            val take = Intent(MediaStore.ACTION_IMAGE_CAPTURE).putExtra(MediaStore.EXTRA_OUTPUT, uri)
                                .addFlags(Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
                            try {
                                @Suppress("DEPRECATION")
                                startActivityForResult(take, 4100)
                            } catch (e: android.content.ActivityNotFoundException) {
                                // no camera app (an emulator): the caller picks a picture instead
                                contentResolver.delete(uri, null, null)
                                photoResult = null
                                result.success("nocamera")
                            }
                        }
                    }
                    "play" -> {
                        player?.release()
                        player = MediaPlayer().apply {
                            setDataSource(call.arguments as String)
                            setOnCompletionListener { it.release(); if (player == it) player = null }
                            prepare()
                            start()
                        }
                        result.success(true)
                    }
                    "saveFolder" -> {
                        // Export: a folder of files (notes, their Attachments/) into one the person picks
                        val a = call.arguments as Map<*, *>
                        treeResult?.success(null)
                        tree = File(a["dir"] as String) to (a["name"] as String)
                        treeResult = result
                        @Suppress("DEPRECATION")
                        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), 4101)
                    }
                    "syncStart" -> {
                        // a sync began: the foreground service (and, once, Android 13's notification permission)
                        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED && !asked) {
                            asked = true
                            requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 4103)
                        }
                        result.success(SyncService.start(this, call.arguments as String? ?: "Syncing…"))
                    }
                    "syncUpdate" -> {
                        val a = call.arguments as Map<*, *>
                        SyncService.update(this, a["text"] as String? ?: "Syncing…", (a["done"] as Number?)?.toInt() ?: 0, (a["total"] as Number?)?.toInt() ?: 0)
                        result.success(true)
                    }
                    "syncStop" -> {
                        SyncService.stop(this)
                        result.success(true)
                    }
                    "keepAwake" -> {
                        // a sync running: the screen stays on (a first sync of a big library takes a while)
                        if (call.arguments == true) window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                        else window.clearFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
                        result.success(true)
                    }
                    "print" -> {
                        // Print (Ctrl+P): the note's PDF to Android's printing
                        val a = call.arguments as Map<*, *>
                        printPdf(File(a["path"] as String), a["name"] as String? ?: "Note")
                        result.success(true)
                    }
                    "saveFile" -> {
                        // a big file (a .bnotes library) where the person chooses, copied as a stream
                        val a = call.arguments as Map<*, *>
                        savingResult?.success(null)
                        saving = File(a["path"] as String)
                        savingResult = result
                        @Suppress("DEPRECATION")
                        startActivityForResult(
                            Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE)
                                .setType(a["mime"] as String? ?: "application/octet-stream")
                                .putExtra(Intent.EXTRA_TITLE, a["name"] as String),
                            4102,
                        )
                    }
                    "stopPlaying" -> {
                        player?.release()
                        player = null
                        result.success(true)
                    }
                    else -> result.notImplemented()
                }
            } catch (e: Exception) {
                result.success(null)
            }
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode == 4101) return savedFolder(resultCode, data)
        if (requestCode == 4102) {
            val r = savingResult ?: return
            val from = saving ?: return
            savingResult = null
            saving = null
            val to = data?.data
            if (resultCode != RESULT_OK || to == null) return r.success(null)
            return background(r) {
                contentResolver.openOutputStream(to)?.use { o -> from.inputStream().use { i -> i.copyTo(o) } }
                true
            }
        }
        if (requestCode != 4100) return
        val r = photoResult ?: return
        photoResult = null
        val uri = photo ?: return r.success(null)
        if (resultCode != RESULT_OK) {
            contentResolver.delete(uri, null, null)
            return r.success(null)
        }
        try {
            val out = File(cacheDir, "Photo ${System.currentTimeMillis()}.jpg")
            contentResolver.openInputStream(uri)?.use { i -> out.outputStream().use { o -> i.copyTo(o) } }
            // Settings > Save to Photos off (and scans): only the note keeps it
            if (!photoKeep) contentResolver.delete(uri, null, null)
            r.success(out.path)
        } catch (e: Exception) {
            r.success(null)
        }
    }

    /** The folder picked for saveFolder: a folder named as asked made in it (a number after the
     *  name when there is one already), and the files copied in. Answers how many files. */
    private fun savedFolder(resultCode: Int, data: Intent?) {
        val r = treeResult ?: return
        val (src, name) = tree ?: return
        treeResult = null
        tree = null
        val picked = data?.data
        if (resultCode != RESULT_OK || picked == null) return r.success(null)
        background(r) {
            val root = DocumentsContract.buildDocumentUriUsingTree(picked, DocumentsContract.getTreeDocumentId(picked))
            val dir = DocumentsContract.createDocument(contentResolver, root, DocumentsContract.Document.MIME_TYPE_DIR, name) ?: return@background null
            copyInto(src, dir)
        }
    }

    private fun copyInto(from: File, to: Uri): Int {
        var n = 0
        for (f in from.listFiles()?.sortedBy { it.name } ?: return 0) {
            if (f.isDirectory) {
                val d = DocumentsContract.createDocument(contentResolver, to, DocumentsContract.Document.MIME_TYPE_DIR, f.name) ?: continue
                n += copyInto(f, d)
            } else {
                val mime = android.webkit.MimeTypeMap.getSingleton().getMimeTypeFromExtension(f.extension.lowercase()) ?: "application/octet-stream"
                // (the name as given: some providers add an extension for the type otherwise)
                val doc = DocumentsContract.createDocument(contentResolver, to, mime, f.name) ?: continue
                contentResolver.openOutputStream(doc)?.use { o -> f.inputStream().use { i -> i.copyTo(o) } }
                n++
            }
        }
        return n
    }

    private fun printPdf(pdf: File, name: String) {
        val pm = getSystemService(PRINT_SERVICE) as PrintManager
        pm.print(name, object : PrintDocumentAdapter() {
            override fun onLayout(old: PrintAttributes?, new: PrintAttributes?, cancel: CancellationSignal?, cb: LayoutResultCallback, extras: Bundle?) {
                if (cancel?.isCanceled == true) return cb.onLayoutCancelled()
                cb.onLayoutFinished(PrintDocumentInfo.Builder("$name.pdf").setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).build(), true)
            }

            override fun onWrite(pages: Array<out PageRange>?, out: ParcelFileDescriptor, cancel: CancellationSignal?, cb: WriteResultCallback) {
                try {
                    pdf.inputStream().use { i -> ParcelFileDescriptor.AutoCloseOutputStream(out).use { o -> i.copyTo(o) } }
                    cb.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
                } catch (e: Exception) {
                    cb.onWriteFailed(e.message)
                }
            }
        }, null)
    }

    /** The keys a hardware keyboard has here (keys.dart), for the system's Keyboard Shortcuts
     *  list (Meta+/), as the desktop's Help > Keyboard Shortcuts lists them. */
    override fun onProvideKeyboardShortcuts(data: MutableList<KeyboardShortcutGroup>, menu: Menu?, deviceId: Int) {
        super.onProvideKeyboardShortcuts(data, menu, deviceId)
        val c = KeyEvent.META_CTRL_ON
        val cs = KeyEvent.META_CTRL_ON or KeyEvent.META_SHIFT_ON
        fun k(label: String, key: Int, mods: Int = 0) = KeyboardShortcutInfo(label, key, mods)
        data.add(KeyboardShortcutGroup("Notes", listOf(
            k("New Note", KeyEvent.KEYCODE_N, c),
            k("New Folder", KeyEvent.KEYCODE_N, cs),
            k("Duplicate Note", KeyEvent.KEYCODE_D, c),
            k("Find in Note", KeyEvent.KEYCODE_F, c),
            k("Note List Search", KeyEvent.KEYCODE_F, c or KeyEvent.META_ALT_ON),
            k("Print", KeyEvent.KEYCODE_P, c),
            k("Undo (the library's, outside the note)", KeyEvent.KEYCODE_Z, c),
            k("Redo", KeyEvent.KEYCODE_Z, cs),
        )))
        data.add(KeyboardShortcutGroup("Note List", listOf(
            k("Previous Note", KeyEvent.KEYCODE_DPAD_UP),
            k("Next Note", KeyEvent.KEYCODE_DPAD_DOWN),
            k("First Note", KeyEvent.KEYCODE_MOVE_HOME),
            k("Last Note", KeyEvent.KEYCODE_MOVE_END),
            k("A Page Up", KeyEvent.KEYCODE_PAGE_UP),
            k("A Page Down", KeyEvent.KEYCODE_PAGE_DOWN),
            k("Extend the Selection", KeyEvent.KEYCODE_DPAD_DOWN, KeyEvent.META_SHIFT_ON),
            k("Select All Notes", KeyEvent.KEYCODE_A, c),
            k("Delete the Selection", KeyEvent.KEYCODE_FORWARD_DEL),
        )))
        data.add(KeyboardShortcutGroup("Format", listOf(
            k("Title", KeyEvent.KEYCODE_T, cs),
            k("Heading", KeyEvent.KEYCODE_H, cs),
            k("Subheading", KeyEvent.KEYCODE_J, cs),
            k("Body", KeyEvent.KEYCODE_B, cs),
            k("Checklist", KeyEvent.KEYCODE_L, cs),
            k("Bulleted List", KeyEvent.KEYCODE_7, cs),
            k("Dashed List", KeyEvent.KEYCODE_8, cs),
            k("Numbered List", KeyEvent.KEYCODE_9, cs),
            k("Add Link", KeyEvent.KEYCODE_K, c),
        )))
        data.add(KeyboardShortcutGroup("View", listOf(
            k("as List", KeyEvent.KEYCODE_1, c),
            k("as Gallery", KeyEvent.KEYCODE_2, c),
            k("Attachments", KeyEvent.KEYCODE_3, c),
            k("Zoom In", KeyEvent.KEYCODE_PERIOD, cs),
            k("Zoom Out", KeyEvent.KEYCODE_COMMA, cs),
            k("Actual Size", KeyEvent.KEYCODE_0, cs),
        )))
    }
}
