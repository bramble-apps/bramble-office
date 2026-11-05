package app.bramble.notes

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.IBinder
import android.os.PowerManager

/** While a WebDAV sync runs (it runs in the app's own process, on a Dart worker): a foreground
 *  service with a "Syncing 312 of 1,418" notification, holding the CPU (and Wi-Fi) awake, so a
 *  long first sync goes on with the screen off or Notes in the background. Started, updated and
 *  stopped from sync.dart through MainActivity's channel. */
class SyncService : Service() {
    private var wake: PowerManager.WakeLock? = null
    private var wifi: WifiManager.WifiLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val n = notification(this, intent?.getStringExtra("text") ?: "Syncing…", intent?.getIntExtra("done", 0) ?: 0, intent?.getIntExtra("total", 0) ?: 0)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) startForeground(ID, n, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC) else startForeground(ID, n)
        if (wake == null) {
            wake = (getSystemService(POWER_SERVICE) as PowerManager).newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "notes:sync").apply {
                // (a sync can't hold it forever: an hour, then the phone may sleep again)
                acquire(60 * 60 * 1000L)
            }
            @Suppress("DEPRECATION")
            wifi = (applicationContext.getSystemService(WIFI_SERVICE) as WifiManager).createWifiLock(WifiManager.WIFI_MODE_FULL_HIGH_PERF, "notes:sync").apply {
                setReferenceCounted(false)
                acquire()
            }
        }
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        wake?.takeIf { it.isHeld }?.release()
        wifi?.takeIf { it.isHeld }?.release()
        wake = null
        wifi = null
        super.onDestroy()
    }

    companion object {
        private const val ID = 4200
        private const val CHANNEL = "sync"

        fun notification(ctx: Context, text: String, done: Int, total: Int): Notification {
            val nm = ctx.getSystemService(NOTIFICATION_SERVICE) as NotificationManager
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && nm.getNotificationChannel(CHANNEL) == null) {
                nm.createNotificationChannel(NotificationChannel(CHANNEL, "Sync", NotificationManager.IMPORTANCE_LOW).apply { description = "While Notes syncs with your WebDAV server" })
            }
            val open = PendingIntent.getActivity(ctx, 0, Intent(ctx, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE)
            @Suppress("DEPRECATION")
            val b = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) Notification.Builder(ctx, CHANNEL) else Notification.Builder(ctx)
            return b.setSmallIcon(android.R.drawable.stat_notify_sync)
                .setContentTitle("Syncing Notes")
                .setContentText(text)
                .setOngoing(true)
                .setOnlyAlertOnce(true)
                .setContentIntent(open)
                .setProgress(total.coerceAtLeast(0), done.coerceIn(0, total.coerceAtLeast(0)), total <= 0)
                .build()
        }

        /** A sync began: into the foreground (false when Android refuses, e.g. from the background). */
        fun start(ctx: Context, text: String): Boolean = try {
            val i = Intent(ctx, SyncService::class.java).putExtra("text", text)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) ctx.startForegroundService(i) else ctx.startService(i)
            true
        } catch (e: Exception) {
            false
        }

        /** How far it is, on the notification. */
        fun update(ctx: Context, text: String, done: Int, total: Int) {
            (ctx.getSystemService(NOTIFICATION_SERVICE) as NotificationManager).notify(ID, notification(ctx, text, done, total))
        }

        fun stop(ctx: Context) {
            ctx.stopService(Intent(ctx, SyncService::class.java))
        }
    }
}
