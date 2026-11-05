package app.bramble.notes

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.ColorMatrix
import android.graphics.ColorMatrixColorFilter
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.pdf.PdfDocument
import android.graphics.pdf.PdfRenderer
import android.os.ParcelFileDescriptor
import java.io.File
import java.io.FileOutputStream
import kotlin.math.hypot
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/// Documents: a photo's page found (its four corners), squared up and filtered as Notes'
/// scanner does (Color, Grayscale, Black & White, Photo), scans put together as a PDF, and
/// PDFs read (their pages' sizes) and drawn page by page for the viewer.
object Scan {
    private fun decode(path: String, maxSide: Int): Bitmap? {
        val o = BitmapFactory.Options().apply { inJustDecodeBounds = true }
        BitmapFactory.decodeFile(path, o)
        var sample = 1
        while (max(o.outWidth, o.outHeight) / (sample * 2) >= maxSide) sample *= 2
        val b = BitmapFactory.decodeFile(path, BitmapFactory.Options().apply { inSampleSize = sample }) ?: return null
        return rotated(path, b)
    }

    /// The picture upright, as the camera's EXIF says.
    private fun rotated(path: String, b: Bitmap): Bitmap {
        val deg = try {
            when (android.media.ExifInterface(path).getAttributeInt(android.media.ExifInterface.TAG_ORIENTATION, 1)) {
                6 -> 90f; 3 -> 180f; 8 -> 270f; else -> 0f
            }
        } catch (e: Exception) { 0f }
        if (deg == 0f) return b
        return Bitmap.createBitmap(b, 0, 0, b.width, b.height, Matrix().apply { postRotate(deg) }, true)
    }

    /// The page's corners (top left, top right, bottom right, bottom left) as 0..1 of the picture:
    /// the largest bright region (paper on a darker table), at a small size; else a margin in.
    fun detect(path: String): List<Double> {
        val b = decode(path, 320) ?: return listOf(0.05, 0.05, 0.95, 0.05, 0.95, 0.95, 0.05, 0.95)
        val w = b.width; val h = b.height
        val px = IntArray(w * h); b.getPixels(px, 0, w, 0, 0, w, h)
        val g = IntArray(w * h) { val c = px[it]; ((c shr 16 and 255) * 30 + (c shr 8 and 255) * 59 + (c and 255) * 11) / 100 }
        // Otsu's threshold
        val hist = IntArray(256); for (v in g) hist[v]++
        var sum = 0.0; for (i in 0..255) sum += i * hist[i].toDouble()
        var sumB = 0.0; var wB = 0; var best = 0.0; var t = 128
        for (i in 0..255) {
            wB += hist[i]; if (wB == 0) continue
            val wF = g.size - wB; if (wF == 0) break
            sumB += i * hist[i].toDouble()
            val mB = sumB / wB; val mF = (sum - sumB) / wF
            val between = wB.toDouble() * wF * (mB - mF) * (mB - mF)
            if (between > best) { best = between; t = i }
        }
        // the largest bright component
        val label = IntArray(w * h); var bestN = 0; var bestL = 0; var next = 1
        val stack = IntArray(w * h)
        for (s in 0 until w * h) {
            if (g[s] <= t || label[s] != 0) continue
            var sp = 0; stack[sp++] = s; label[s] = next; var n = 0
            while (sp > 0) {
                val p = stack[--sp]; n++
                val x = p % w; val y = p / w
                if (x > 0 && label[p - 1] == 0 && g[p - 1] > t) { label[p - 1] = next; stack[sp++] = p - 1 }
                if (x < w - 1 && label[p + 1] == 0 && g[p + 1] > t) { label[p + 1] = next; stack[sp++] = p + 1 }
                if (y > 0 && label[p - w] == 0 && g[p - w] > t) { label[p - w] = next; stack[sp++] = p - w }
                if (y < h - 1 && label[p + w] == 0 && g[p + w] > t) { label[p + w] = next; stack[sp++] = p + w }
            }
            if (n > bestN) { bestN = n; bestL = next }
            next++
        }
        if (bestN < w * h / 12) return listOf(0.05, 0.05, 0.95, 0.05, 0.95, 0.95, 0.05, 0.95)
        // its corners: the extremes of x+y and x-y
        var tl = 0; var tr = 0; var br = 0; var bl = 0
        var vtl = Int.MAX_VALUE; var vtr = Int.MIN_VALUE; var vbr = Int.MIN_VALUE; var vbl = Int.MAX_VALUE
        for (p in 0 until w * h) {
            if (label[p] != bestL) continue
            val x = p % w; val y = p / w
            if (x + y < vtl) { vtl = x + y; tl = p }
            if (x + y > vbr) { vbr = x + y; br = p }
            if (x - y > vtr) { vtr = x - y; tr = p }
            if (x - y < vbl) { vbl = x - y; bl = p }
        }
        fun pt(p: Int) = listOf((p % w + 0.5) / w, (p / w + 0.5) / h)
        return pt(tl) + pt(tr) + pt(br) + pt(bl)
    }

    /// The page squared up from its corners and filtered, as a JPEG at `out`; its size.
    fun warp(path: String, corners: List<Double>, filter: String, out: String): List<Int> {
        val b = decode(path, 2600) ?: return listOf(0, 0)
        val w = b.width.toFloat(); val h = b.height.toFloat()
        val src = FloatArray(8) { (corners[it] * if (it % 2 == 0) w else h).toFloat() }
        val top = hypot((src[2] - src[0]).toDouble(), (src[3] - src[1]).toDouble())
        val bottom = hypot((src[4] - src[6]).toDouble(), (src[5] - src[7]).toDouble())
        val left = hypot((src[6] - src[0]).toDouble(), (src[7] - src[1]).toDouble())
        val right = hypot((src[4] - src[2]).toDouble(), (src[5] - src[3]).toDouble())
        var ow = max(top, bottom); var oh = max(left, right)
        val k = min(1.0, 2400.0 / max(ow, oh)); ow *= k; oh *= k
        val W = max(16, ow.roundToInt()); val H = max(16, oh.roundToInt())
        val dst = floatArrayOf(0f, 0f, W.toFloat(), 0f, W.toFloat(), H.toFloat(), 0f, H.toFloat())
        val m = Matrix(); m.setPolyToPoly(src, 0, dst, 0, 4)
        val outB = Bitmap.createBitmap(W, H, Bitmap.Config.ARGB_8888)
        val c = Canvas(outB); c.drawColor(android.graphics.Color.WHITE)
        val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
        paint.colorFilter = filterOf(filter)
        c.concat(m); c.drawBitmap(b, 0f, 0f, paint)
        FileOutputStream(out).use { outB.compress(Bitmap.CompressFormat.JPEG, 88, it) }
        return listOf(W, H)
    }

    private fun filterOf(f: String): ColorMatrixColorFilter? {
        fun contrast(k: Float, bright: Float) = ColorMatrix(floatArrayOf(k, 0f, 0f, 0f, bright, 0f, k, 0f, 0f, bright, 0f, 0f, k, 0f, bright, 0f, 0f, 0f, 1f, 0f))
        return when (f) {
            "photo" -> null
            "grayscale" -> ColorMatrixColorFilter(ColorMatrix().apply { setSaturation(0f); postConcat(contrast(1.25f, -20f)) })
            "bw" -> ColorMatrixColorFilter(ColorMatrix().apply { setSaturation(0f); postConcat(contrast(3.2f, -300f)) })
            else -> ColorMatrixColorFilter(ColorMatrix().apply { setSaturation(1.15f); postConcat(contrast(1.3f, -25f)) })
        }
    }

    /// A PDF of the pages (JPEGs), each 612 points wide; the pages' sizes in points.
    fun pdf(pages: List<String>, out: String): List<List<Double>> {
        val doc = PdfDocument()
        val sizes = mutableListOf<List<Double>>()
        for ((i, p) in pages.withIndex()) {
            val b = BitmapFactory.decodeFile(p) ?: continue
            val pw = 612; val ph = (612.0 * b.height / b.width).roundToInt()
            val page = doc.startPage(PdfDocument.PageInfo.Builder(pw, ph, i + 1).create())
            page.canvas.drawBitmap(b, null, android.graphics.Rect(0, 0, pw, ph), Paint(Paint.FILTER_BITMAP_FLAG))
            doc.finishPage(page)
            sizes.add(listOf(pw.toDouble(), ph.toDouble()))
        }
        FileOutputStream(out).use { doc.writeTo(it) }
        doc.close()
        return sizes
    }

    /// A PDF's pages' sizes in points.
    fun pdfInfo(path: String): List<List<Double>> {
        ParcelFileDescriptor.open(File(path), ParcelFileDescriptor.MODE_READ_ONLY).use { fd ->
            PdfRenderer(fd).use { r ->
                return (0 until r.pageCount).map { i -> r.openPage(i).use { listOf(it.width.toDouble(), it.height.toDouble()) } }
            }
        }
    }

    /// Page `index` drawn `width` pixels wide as a PNG at `out`.
    fun pdfRender(path: String, index: Int, width: Int, out: String): Boolean {
        ParcelFileDescriptor.open(File(path), ParcelFileDescriptor.MODE_READ_ONLY).use { fd ->
            PdfRenderer(fd).use { r ->
                if (index !in 0 until r.pageCount) return false
                r.openPage(index).use { pg ->
                    val h = max(1, (width.toDouble() * pg.height / pg.width).roundToInt())
                    val b = Bitmap.createBitmap(width, h, Bitmap.Config.ARGB_8888)
                    b.eraseColor(android.graphics.Color.WHITE)
                    pg.render(b, null, null, PdfRenderer.Page.RENDER_MODE_FOR_DISPLAY)
                    FileOutputStream(out).use { b.compress(Bitmap.CompressFormat.PNG, 100, it) }
                }
            }
        }
        return true
    }
}
