package app.bramble;

import android.app.NativeActivity;
import android.content.ClipData;
import android.content.Intent;
import android.content.SharedPreferences;
import android.database.Cursor;
import android.net.Uri;
import android.os.Bundle;
import android.os.FileObserver;
import android.provider.DocumentsContract;
import android.provider.OpenableColumns;
import android.util.Log;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.Map;

/**
 * The activity Keydeck, Values and Letters run in: Android's NativeActivity (the Rust app draws
 * everything) plus what Rust can't reach on its own: the system file picker, "Open with" and web
 * links (kn-ui's android.rs calls these).
 *
 * A picked or opened file is copied into the app's own folder and handed to the app; whenever the
 * app saves that copy, it is written back to where it came from. A save goes to a new file in the
 * app's folder at once, and is copied to the place the picker gives (again on every later save).
 */
public class BrambleActivity extends NativeActivity {
    private static final String TAG = "Bramble";
    private static final int OPEN = 1, SAVE = 2, FOLDER = 3;

    private final ArrayList<String> results = new ArrayList<>();
    /** The app's copies and where they belong (kept across launches). */
    private final HashMap<String, Uri> links = new HashMap<>();
    private final HashMap<String, FileObserver> watchers = new HashMap<>();
    private String pendingSave, pendingFolder;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        if (state != null) {
            pendingSave = state.getString("pendingSave");
            pendingFolder = state.getString("pendingFolder");
        }
        restoreLinks();
        if (state == null) handle(getIntent());
    }

    @Override
    protected void onSaveInstanceState(Bundle state) {
        super.onSaveInstanceState(state);
        state.putString("pendingSave", pendingSave);
        state.putString("pendingFolder", pendingFolder);
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        handle(intent);
    }

    /** Back doesn't quit (and lose what's open): the app goes to the background. */
    @Override
    public void onBackPressed() {
        moveTaskToBack(true);
    }

    /** "Open with" and "Share to" this app. */
    private void handle(Intent intent) {
        if (intent == null) return;
        Uri uri = null;
        String action = intent.getAction();
        if (Intent.ACTION_VIEW.equals(action) || Intent.ACTION_EDIT.equals(action)) uri = intent.getData();
        else if (Intent.ACTION_SEND.equals(action)) uri = intent.getParcelableExtra(Intent.EXTRA_STREAM);
        if (uri == null) return;
        final Uri u = uri;
        final boolean writable = (intent.getFlags() & Intent.FLAG_GRANT_WRITE_URI_PERMISSION) != 0;
        new Thread(() -> bringIn(u, writable)).start();
    }

    // ---- called from Rust (on the app's thread) ----

    public void pickOpen(String mime, boolean multiple) {
        runOnUiThread(() -> {
            Intent i = new Intent(Intent.ACTION_OPEN_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType(mime);
            i.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, multiple);
            start(i, OPEN);
        });
    }

    public void pickSave(String path) {
        runOnUiThread(() -> {
            pendingSave = path;
            Intent i = new Intent(Intent.ACTION_CREATE_DOCUMENT);
            i.addCategory(Intent.CATEGORY_OPENABLE);
            i.setType(mimeOf(path));
            i.putExtra(Intent.EXTRA_TITLE, new File(path).getName());
            start(i, SAVE);
        });
    }

    public void pickFolder(String path) {
        runOnUiThread(() -> {
            pendingFolder = path;
            start(new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE), FOLDER);
        });
    }

    public void openUrl(String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (Exception e) {
                Log.w(TAG, "open " + url, e);
            }
        });
    }

    /** Files ready for the app since the last call, one path per line (null: none). */
    public synchronized String takeResults() {
        if (results.isEmpty()) return null;
        String s = String.join("\n", results);
        results.clear();
        return s;
    }

    private synchronized void deliver(String path) {
        results.add(path);
    }

    private void start(Intent i, int code) {
        try {
            startActivityForResult(i, code);
        } catch (Exception e) {
            Log.e(TAG, "no file picker", e);
        }
    }

    @Override
    protected void onActivityResult(int code, int result, Intent data) {
        super.onActivityResult(code, result, data);
        boolean ok = result == RESULT_OK && data != null;
        if (code == OPEN && ok) {
            ArrayList<Uri> uris = new ArrayList<>();
            ClipData clip = data.getClipData();
            if (clip != null) for (int k = 0; k < clip.getItemCount(); k++) uris.add(clip.getItemAt(k).getUri());
            else if (data.getData() != null) uris.add(data.getData());
            new Thread(() -> {
                for (Uri u : uris) bringIn(u, keep(u));
            }).start();
        } else if (code == SAVE) {
            String path = pendingSave;
            pendingSave = null;
            if (ok && path != null && data.getData() != null) {
                Uri u = data.getData();
                keep(u);
                link(path, u);
                new Thread(() -> {
                    if (new File(path).isFile()) copyOut(path, u);
                }).start();
            }
        } else if (code == FOLDER) {
            String path = pendingFolder;
            pendingFolder = null;
            if (ok && path != null && data.getData() != null) {
                Uri tree = data.getData();
                keep(tree);
                new Thread(() -> mirror(new File(path), tree)).start();
            }
        }
    }

    /** Keeps access to a picked place across launches; whether it can be written. */
    private boolean keep(Uri u) {
        int read = Intent.FLAG_GRANT_READ_URI_PERMISSION, write = Intent.FLAG_GRANT_WRITE_URI_PERMISSION;
        try {
            getContentResolver().takePersistableUriPermission(u, read | write);
            return true;
        } catch (Exception e) {
            try {
                getContentResolver().takePersistableUriPermission(u, read);
            } catch (Exception ignored) {
            }
            return false;
        }
    }

    /** Copies a picked file into the app's folder and hands it to the app. */
    private void bringIn(Uri uri, boolean writable) {
        try {
            File dir = new File(getFilesDir(), "open/" + Integer.toHexString(uri.toString().hashCode()));
            dir.mkdirs();
            File f = new File(dir, displayName(uri));
            try (InputStream in = getContentResolver().openInputStream(uri); OutputStream out = new FileOutputStream(f)) {
                copy(in, out);
            }
            if (writable) link(f.getPath(), uri);
            deliver(f.getPath());
        } catch (Exception e) {
            Log.e(TAG, "open " + uri, e);
        }
    }

    // ---- writing copies back ----

    private synchronized void link(String path, Uri uri) {
        links.put(path, uri);
        saveLinks();
        watch(new File(path).getParent());
    }

    private synchronized Uri linkFor(String path) {
        return links.get(path);
    }

    /** Watches a folder of the app's copies: a copy that's saved goes back to its place. */
    private synchronized void watch(String dir) {
        if (dir == null || watchers.containsKey(dir)) return;
        FileObserver o = new FileObserver(dir, FileObserver.CLOSE_WRITE | FileObserver.MOVED_TO) {
            @Override
            public void onEvent(int event, String name) {
                if (name == null) return;
                String path = dir + "/" + name;
                Uri u = linkFor(path);
                if (u != null) copyOut(path, u);
            }
        };
        o.startWatching();
        watchers.put(dir, o);
    }

    private void copyOut(String path, Uri uri) {
        for (String mode : new String[] {"wt", "w"}) {
            try (InputStream in = new FileInputStream(path); OutputStream out = getContentResolver().openOutputStream(uri, mode)) {
                copy(in, out);
                return;
            } catch (Exception e) {
                if (mode.equals("w")) Log.e(TAG, "save to " + uri, e);
            }
        }
    }

    /** A folder the app is writing into (exported images): copies its files into the picked
     * folder as they appear, for ten minutes. */
    private void mirror(File dir, Uri tree) {
        Uri root = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree));
        HashMap<String, Long> copied = new HashMap<>(), seen = new HashMap<>();
        HashMap<String, Uri> folders = new HashMap<>();
        folders.put("", root);
        long until = System.currentTimeMillis() + 10 * 60 * 1000;
        while (System.currentTimeMillis() < until) {
            try {
                mirrorDir(dir, "", folders, copied, seen);
                Thread.sleep(2000);
            } catch (Exception e) {
                Log.e(TAG, "export to " + tree, e);
                return;
            }
        }
    }

    private void mirrorDir(File dir, String rel, HashMap<String, Uri> folders, HashMap<String, Long> copied, HashMap<String, Long> seen) throws Exception {
        File[] files = dir.listFiles();
        if (files == null) return;
        for (File f : files) {
            String r = rel.isEmpty() ? f.getName() : rel + "/" + f.getName();
            if (f.isDirectory()) {
                if (!folders.containsKey(r)) {
                    Uri made = DocumentsContract.createDocument(getContentResolver(), folders.get(rel), DocumentsContract.Document.MIME_TYPE_DIR, f.getName());
                    if (made == null) continue;
                    folders.put(r, made);
                }
                mirrorDir(f, r, folders, copied, seen);
                continue;
            }
            // (a file goes once it has stopped changing)
            long stamp = f.lastModified() * 31 + f.length();
            Long before = seen.put(r, stamp);
            if (before == null || before != stamp || Long.valueOf(stamp).equals(copied.get(r))) continue;
            Uri doc = DocumentsContract.createDocument(getContentResolver(), folders.get(rel), mimeOf(f.getName()), f.getName());
            if (doc != null) copyOut(f.getPath(), doc);
            copied.put(r, stamp);
        }
    }

    private void saveLinks() {
        SharedPreferences.Editor e = getSharedPreferences("links", MODE_PRIVATE).edit().clear();
        for (Map.Entry<String, Uri> l : links.entrySet()) e.putString(l.getKey(), l.getValue().toString());
        e.apply();
    }

    private synchronized void restoreLinks() {
        for (Map.Entry<String, ?> l : getSharedPreferences("links", MODE_PRIVATE).getAll().entrySet()) {
            if (new File(l.getKey()).isFile()) {
                links.put(l.getKey(), Uri.parse(String.valueOf(l.getValue())));
                watch(new File(l.getKey()).getParent());
            }
        }
    }

    // ---- helpers ----

    private String displayName(Uri uri) {
        String name = null;
        try (Cursor c = getContentResolver().query(uri, new String[] {OpenableColumns.DISPLAY_NAME}, null, null, null)) {
            if (c != null && c.moveToFirst()) name = c.getString(0);
        } catch (Exception ignored) {
        }
        if (name == null) name = uri.getLastPathSegment();
        if (name == null || name.isEmpty()) name = "file";
        return name.replace('/', '-');
    }

    private static String mimeOf(String path) {
        String name = new File(path).getName();
        int dot = name.lastIndexOf('.');
        String ext = dot < 0 ? "" : name.substring(dot + 1).toLowerCase();
        String m = MimeTypeMap.getSingleton().getMimeTypeFromExtension(ext);
        return m != null ? m : "application/octet-stream";
    }

    private static void copy(InputStream in, OutputStream out) throws java.io.IOException {
        byte[] buf = new byte[1 << 16];
        for (int n; (n = in.read(buf)) > 0; ) out.write(buf, 0, n);
    }
}
