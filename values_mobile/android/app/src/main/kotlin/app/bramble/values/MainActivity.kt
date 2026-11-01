package app.bramble.values

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.CancellationSignal
import android.os.ParcelFileDescriptor
import android.print.PageRange
import android.print.PrintAttributes
import android.print.PrintDocumentAdapter
import android.print.PrintDocumentInfo
import android.print.PrintManager
import android.speech.tts.TextToSpeech
import android.webkit.MimeTypeMap
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.io.FileOutputStream

class MainActivity : FlutterActivity() {
    private var tts: TextToSpeech? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "app.bramble.values/open").setMethodCallHandler { call, result ->
            when (call.method) {
                // web pages the app opens (Values Help, Support Development…): the system's browser
                // (a file of the app's own: Values Help's page, a movie: through the FileProvider)
                "url" -> try {
                    val url = call.arguments as String
                    val intent = if (url.startsWith("/")) {
                        val f = File(url)
                        val type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(f.extension.lowercase()) ?: "*/*"
                        Intent(Intent.ACTION_VIEW).setDataAndType(FileProvider.getUriForFile(this, "app.bramble.values.files", f), type).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                    } else {
                        Intent(Intent.ACTION_VIEW, Uri.parse(url))
                    }
                    startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                    result.success(true)
                } catch (e: Exception) {
                    android.util.Log.w("Values", "couldn't open ${call.arguments}: $e")
                    result.success(false)
                }
                // Edit > Speech > Start Speaking (the text) / Stop Speaking ("")
                "speak" -> {
                    val text = call.arguments as String
                    if (text.isEmpty()) {
                        tts?.stop()
                    } else {
                        val t = tts
                        if (t == null) {
                            var made: TextToSpeech? = null
                            made = TextToSpeech(this) { status -> if (status == TextToSpeech.SUCCESS) made?.speak(text, TextToSpeech.QUEUE_FLUSH, null, "values") }
                            tts = made
                        } else {
                            t.speak(text, TextToSpeech.QUEUE_FLUSH, null, "values")
                        }
                    }
                    result.success(true)
                }
                // File > Print…: the exported PDF through the system's print service
                "print" -> try {
                    val args = call.arguments as List<*>
                    printPdf(File(args[0] as String), args[1] as String)
                    result.success(true)
                } catch (e: Exception) {
                    result.success(false)
                }
                else -> result.notImplemented()
            }
        }
    }

    override fun onDestroy() {
        tts?.shutdown()
        super.onDestroy()
    }

    private fun printPdf(pdf: File, name: String) {
        val adapter = object : PrintDocumentAdapter() {
            override fun onLayout(old: PrintAttributes?, new: PrintAttributes?, cancel: CancellationSignal?, callback: LayoutResultCallback, extras: Bundle?) {
                if (cancel?.isCanceled == true) return callback.onLayoutCancelled()
                callback.onLayoutFinished(PrintDocumentInfo.Builder(name).setContentType(PrintDocumentInfo.CONTENT_TYPE_DOCUMENT).build(), true)
            }

            override fun onWrite(pages: Array<out PageRange>?, dest: ParcelFileDescriptor, cancel: CancellationSignal?, callback: WriteResultCallback) {
                try {
                    pdf.inputStream().use { input -> FileOutputStream(dest.fileDescriptor).use { input.copyTo(it) } }
                    callback.onWriteFinished(arrayOf(PageRange.ALL_PAGES))
                } catch (e: Exception) {
                    callback.onWriteFailed(e.message)
                }
            }
        }
        (getSystemService(Context.PRINT_SERVICE) as PrintManager).print(name, adapter, null)
    }
}
