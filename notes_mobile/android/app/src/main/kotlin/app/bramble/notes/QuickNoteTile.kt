package app.bramble.notes

import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.TileService

/// Quick Settings' Quick Note: opens Notes on a new Quick Note (or the last one, as Settings says).
class QuickNoteTile : TileService() {
    override fun onClick() {
        super.onClick()
        val open = Intent(this, MainActivity::class.java)
            .setAction("app.bramble.notes.QUICK_NOTE")
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (Build.VERSION.SDK_INT >= 34) {
            startActivityAndCollapse(PendingIntent.getActivity(this, 0, open, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT))
        } else {
            @Suppress("DEPRECATION", "StartActivityAndCollapseDeprecated")
            startActivityAndCollapse(open)
        }
    }
}
