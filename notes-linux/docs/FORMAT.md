# Bramble Notes' library format

Bramble Notes keeps a library as plain files. The same layout lives on disk and on a WebDAV sync
server: syncing copies files, and each file holds one note. The code is `nt-core/src/native.rs`;
the note JSON is in `nt-core/src/sync.rs`.

```
Bramble Notes/                 ~/.local/share/bramble-notes/ (the app's library)
  library.json                 folders, Smart Folders, tags, format version
  notes/<id>.json              one per note
  files/<sha256>               attachment files, named by their content
  index/                       a cache: rebuilt from the files when missing; never synced
    NoteStore.sqlite           a database in Apple's NoteStore format, read for the list and notes
    Accounts/…                 its attachment files (hard links into files/)
    manifest.json              what of the files the database holds
  sync.json                    the sync account (readable by the user only)
  sync-state.json, sync-base/  what was last in step with the server
```

## library.json

```json
{"format": "bramble-notes", "version": 1,
 "accounts": [{"id": "LocalAccount", "name": "On My Mac"}, {"id": "…", "name": "iCloud"}],
 "folders": [{"id": "…", "name": "Work", "parent": null, "account": "LocalAccount", "smart": null, "order": 2}, …],
 "tags": ["food", …]}
```

- `accounts` are the libraries' accounts, as on the Mac: a Mac's iCloud and On My Mac come in as two accounts (shown as sections in the sidebar), not as folders. `LocalAccount` is this device's own, shown as "On This Device". Every account is local here; none syncs through iCloud.
- `account` is the account a folder belongs to; a folder's subfolders and notes are in the same one.
- `smart` holds a Smart Folder's query in Notes' own JSON (`ZSMARTFOLDERQUERYJSON`).
- The Notes folder and Recently Deleted are always there and aren't listed.

## notes/<id>.json

```json
{"v": 1, "note": {
  "id": "…", "folder": "<folder id>", "created": 1730000000.0, "modified": 1730000000.0,
  "pinned": false, "deleted": false, "locked": false, "title": "…", "snippet": "…",
  "body": "<base64: Notes' note protobuf (gzipped topotext), as in ZICNOTEDATA.ZDATA>",
  "outline": "<base64: collapsed headings, as ZOUTLINESTATEDATA>",
  "inline": [{"id": "…", "alt": "#tag", "content": "TAG"}],
  "attachments": [{
     "id": "…", "uti": "public.png", "name": "photo.png", "width": 320, "height": 200,
     "file": {"sha": "<sha256>", "name": "photo.png"}, "fallback": null,
     "table": "<base64: the table's CRDT, as ZMERGEABLEDATA1>",
     "drawing": "<base64: strokes and Markup shapes>",
     "pages": [[0, 612, 792, [0, 0, 612, 792]]], "view": "Large", "parts": [ … scan pages … ],
     "url": null, "summary": "", "created": 0, "modified": 0, "notDownloaded": false}]}}
```

- Dates are seconds since 1970.
- Every piece of a note's content keeps Notes' own encoding. That covers text, styles, checklists, links, math results, hashtags, tables, collapsed headings and drawings, so nothing is translated away.
- **A locked note** has no `body` and `attachments`. It carries `"sealed": {crypto, data, iv, tag, attachments, inline, files}`, the encrypted rows exactly as Notes stores them (PBKDF2-SHA256, AES key wrap, AES-GCM). Its encrypted files are in `files/` like any other. The note's password opens it on any device; the files never hold its text.
- **A note deleted for good** is a tombstone: `{"v": 1, "deleted": true, "id": "…", "at": 1730000000.0}`. A note in Recently Deleted is an ordinary note whose folder is Recently Deleted.

## files/<sha256>

Attachment files (pictures, PDFs, audio, drawings' fallback pictures, scans' pages, a locked note's encrypted files) are named by the SHA-256 of their content. They're written once and never changed, so syncing uploads each one once.

## The index

The app reads from `index/` and writes to it. Every write is then mirrored to the changed notes' files. Each file is written to a temporary name and renamed into place, and only the changed notes' files are touched.

On open, the index is brought in step with the files:
- note files that changed elsewhere (sync, a copied library) are read in
- tombstones erase notes
- the index's own changes are written out

If `index/` is deleted, it's rebuilt from the files.

## Converting an older library

The first time a library from before this format opens (`NoteStore.sqlite` in the library folder), it's converted in five steps:
1. A full backup goes beside it, as `bramble-notes-backup-<date>`. The database is copied consistently and the attachment files are hard-linked. The backup is never touched afterwards.
2. Notes and attachments are written as files.
3. The database moves into `index/`.
4. The counts of notes, folders, attachments, locked notes and tags are checked, read from the files themselves, against the database.
5. Only then is `library.json` written.

If any step fails, the old layout is put back and the app opens it as before. A library open in another program (an older app still running) isn't converted until that program has closed it.

While it converts, the window shows how far along it is ("Updating your library… 120 of 1186"). If the app quits partway, the next open picks up where it left off. Before step 3 that means starting the conversion again; after step 3 it means finishing it.

Only one process at a time opens or converts a library. Each holds a lock on `.lock` in the library folder while it does, and a second one waits ("Waiting for another Notes window…") until the first is done.

## Exchange

- **`.bnotes`:** the library folder zipped (library.json, notes/, files/), with no index. It's lossless; importing adds the notes not already present, by id.
- **Apple's NoteStore:** importing a Mac's library, and exporting to Apple's format, work as before.
- **Markdown, HTML, PDF and plain text:** these exports read the same notes.

## Sync

The server holds the same files, in the folder chosen in Settings:
- `notes/<id>.json` and `files/<sha256>`
- `meta.json` (folders)
- `stamp.json`, which changes whenever anything does, so a quiet check reads just that one file

Each device remembers every note's hash and ETag as of the last sync, and the version both sides agreed on (`sync-base/`). A note changed on two devices is merged against that version:
- paragraphs by their ids
- attachments and inline tokens by theirs
- folder, pin and Recently Deleted from the side that changed them

Only edits that clash fall back to "the newer edit wins" plus an "(other copy)" note. Clashing means the same paragraph or table changed both ways, an edit against a deletion, or a locked note. Deletions travel as tombstones.

A sync sends and reads up to six files at once, in this order:
1. `meta.json`, so a note never reaches another device before its folder does.
2. Attachments (`files/<sha256>`). A note is sent only after its files are on the server.
3. The notes.
4. `stamp.json`, last, and only once everything else has gone. A sync cut short never looks finished to another device.

When the server asks to slow down (429, 503, a timeout), the sync waits as it asks and tries again. It then runs two transfers at once, then one, for the rest of the sync. What was sent is recorded as it lands (`sync-state.json`), so a sync cut short goes on from there next time. Attachments are streamed, never held in memory whole. An attachment read from the server is checked against its name (its SHA-256). A note file that doesn't read yet is left for the next round.
