# Bramble Notes window: parity with Apple Notes 4.13

What the window around the editor (crates/nt-app: sidebar, list, toolbar, menus, Settings, dialogs,
lock, store, smart folders, import and export) does next to Apple's. The editor itself is in
[PARITY-EDITOR.md](PARITY-EDITOR.md). Sources: the NO1 feature map, menu tree, context menus and
strings (notes-research-src/partial-3/Notes-Research/features), the NO1–NO3 captures.
**done** = built and checked against Apple's captures (or, where none exist, its strings),
**tested** = a unit test or a scripted run checks it, **skipped** = left out, with the reason.
Proprietary services (iCloud sync and sharing, Apple Intelligence, Apple Photos, macOS services) are
skipped by design.

Always on: `menus::tests::menu_audit` lists every menu command and who handles it (shell, editor,
greyed); the menu sweep (every shell command, one run each, on a copy of the user's library) and
monkey sessions (random clicks, drags, scrolls, keys, typing and menu commands) run with the
watchdog on: no panic, no stuck frame (see *Speed and stalls*).

## Window and toolbar
| | status |
|---|---|
| Three panes; toolbar split per pane (sidebar / list title / editor), glass and plain capsules | done (NO2/NO3 pixel rounds) |
| Sidebar slide (hide ~217 ms, show ~250 ms on Apple's curve), resizable sidebar and list, sizes kept | done, tested |
| Editor hairline under the toolbar once the note is scrolled (in ~140 ms, out ~240 ms) | done |
| Toolbar capsules from kn_ui::capsule (shared with Letters, Values, Keydeck): pixel-identical to before with glass off, glass on and classic, light and dark, a lone button hovered | done, tested |
| Customize Toolbar… sheet (all Apple's items, default set, drag in/out); right-click on the toolbar | done, tested (sweep) |
| View > Hide Toolbar, Hide Sidebar, Hide Note Count, Zoom In / Out / Actual Size, Enter Full Screen | done, tested (sweep) |
| Window > Minimize, Zoom, Notes, Notes Settings | done, tested (sweep) |
| Window > Open Note in New Window, a double-click on a note in the list, the note's context menu: a window of its own (own caret, selection, undo), brought forward if open; Window > Float on Top for the one used last | done, tested (scripted: two windows, one floating; pixels of the second window not captured by snapshots) |
| The note scrolling up under the translucent toolbar (Mac: the band blurs what's under it to a ~6 % tint, white at rest) | done, tested (scrolled: an even tint, no glyph readable; at rest pixel-identical to before) |
| Substitutions > Data Detectors tick, Writing Direction ticks following the paragraph | done, tested (NT_DUMP after the commands) |
| Window > Photo Browser | skipped: browses the Apple Photos library |
| Writing Tools, Services, Speech, AutoFill, Start Dictation, Emoji & Symbols, window tiling items | skipped: macOS / Apple Intelligence services (removed from the menus) |

## Sidebar, folders, accounts
| | status |
|---|---|
| Accounts with All …, Notes, Quick Notes, folders, nested folders with disclosure, smart folders (gear), Recently Deleted when not empty, Tags browser | done (NO1–NO3) |
| Material strip under the toolbar (feathered blur, dither), sticky Pinned/date headers in the list | done (NO3 sticky captures) |
| New Folder ⇧⌘N, Rename Folder, Delete Folder (its notes to Recently Deleted), New Folder inside | done, tested (sweep) |
| Drag a folder onto another to nest it; onto its account's header for the top level; not into itself or its own | done, tested (scripted drags, demo and library copy) |
| Drag notes from the list onto a folder (Move) | done, tested (scripted drag) |
| Folder context menu: Rename, Delete, New Folder, Sort By ▸, Group By Date ▸, Convert to Smart Folder (Edit Smart Folder on smart ones) | done |
| Tag chips: select, ⌘-click several, context menu (include / exclude / deselect, match all or any) | done |
| Notes > Accounts… (Internet Accounts) | skipped: macOS accounts pane |
| Shared section, Share Folder / Manage Shared Folder / Show Folder Activity | skipped: iCloud sharing |
| Math Notes, Call Recordings system folders | skipped: Apple Intelligence / phone features (shown when a Mac library has them) |

## Smart Folders
| | status |
|---|---|
| New Smart Folder sheet: Name, all / any, filter rows with +/-: Tags, Date Created, Date Edited, Checklists, Attachments, Folders, Quick Notes, Pinned, Locked | done, tested (`smart::tests`) |
| Reads and writes Apple's query JSON (and the shell's earlier forms); legacy date ranges | done, tested |
| New Smart Folder with Tag Selection; Convert Folder to Smart Folder (refused with subfolders or locked notes); Edit Smart Folder | done, tested (sweep) |
| Counts on the user's library: B-Smart Pinned 6, Past week 92, Past 3 months 92 (as the Mac); Attachments 154 (Mac 157: locked / not-downloaded notes) | tested |
| Checklists and Attachments filters on light notes (no note decoded): nt-core's columns | done |
| Shared, Mentions filters | skipped: iCloud collaboration |

## Note list
| | status |
|---|---|
| Rows: title, time + snippet, folder in All views, thumbnail; Pinned section (collapsible); date groups (Today, Yesterday, Previous 7/30 Days, months, years) | done (NO2/NO3) |
| View > as List ⌘1 / as Gallery ⌘2 (cards with live miniatures); View Options (…) menu | done, tested (sweep) |
| Sort By (Default / Edited / Created / Title, Newest / Oldest First), Group By Date (Default / On / Off), per folder | done, tested (sweep) |
| View > Recent Notes ▸ Previous ⌥⌘[ / Next ⌥⌘] / Clear Menu | done, tested (sweep) |
| Multi-selection: ⇧-click range, ⌘-click (Ctrl) toggle, ⇧↑↓ / ⇧Home / ⇧End extend, ⌘A in the list selects all (in the note: its text); ↑↓ Home End Page Up/Down move; Delete / Backspace deletes the selection; Delete, Pin, Move (menu, drag) and Export act on all of it; the editor shows "N Notes Selected" | done, tested (`tools/shortcut_checks.sh`) |
| Edit > Undo / Redo for the library outside the note ("Undo Delete Note(s)", "Move Notes", "Pin / Unpin Notes", "New Folder", "Delete Folder"): deleted notes back into their folders (pins too), a deleted folder made again with its subfolders and notes; in the note ⌘Z stays the text's | done, tested (demo and a store library, across reopening) |
| Help > Keyboard Shortcuts (a Linux addition): the list's keys and every menu shortcut | done |
| Note List Search ⌥⌘F (all accounts, text in attachments' summaries) | done |
| Note context menu: Pin / Unpin, Lock Note, New Note, Duplicate Note, Tags ▸, Move to ▸ (smart folders disabled), Delete | done |
| Share Note | skipped: iCloud collaboration (the Share toolbar button exports PDF / Markdown) |
| Swipe actions on a row (two-finger horizontal scroll on a trackpad, or a finger on a touch screen): right Pin / Unpin; left Delete, Move, Share (Recently Deleted: Delete, Move); the row follows 1:1, the buttons grow and their names fade in; it snaps open or shut on a spring carrying the release speed (notes_mobile's: stiffness 260, damping 0.92, flick 300 pt/s); fully open is a resting stop (trackpad deltas at 0.7×, a finger 1:1); past it a rubber band (0.3× at first, never reaching the edge), and only the fingers' own travel well past open (65 % of the row's width, at least 120 pt) makes the outermost button snap across and take the row (back below 75 % of that cancels it); coasting after the fingers lift never moves a row; the axis is decided in the first 6 pt, so scrolling never opens a row; a click elsewhere, Escape or a scroll shuts it | done, tested (scripted swipes: open, flick, slow release, fast swipes stop at open, momentum and unmarked coasting tails leave an open row alone, deliberate full left deletes, full right pins, buttons, Move / Share menus, vertical and diagonal scrolls, closing, touch). No Mac capture: looks follow iOS 26 and the Android build |
| Virtualised rows, thumbnails decoded on a worker | done, tested (frames ~3–5 ms on 1230 notes) |

## Attachments browser
| | status |
|---|---|
| View > Show Attachments Browser ⌘3 (and View Options' View Attachments): Photos & Videos, Scans, Maps, Websites, Audio, Documents; "No Attachments / … added to your notes appear here." | done (from Apple's strings: no capture of it yet), tested (library copy: 282 attachments) |
| Read on a worker from the store (own connection) | done |
| Click selects, double-click opens; context menu Show in Note, Quick Look, Open, Save Attachment…, Copy | done, tested (Show in Note goes to the note) |
| Rename Attachment… and Share from the browser | greyed: renaming is the editor's (Rename Attachment… in a note works); Share is Apple's share sheet |

## Notes
| | status |
|---|---|
| New Note ⌘N (Settings' "New notes start with"), Duplicate ⌘D, Pin, Delete (to Recently Deleted) | done, tested (sweep) |
| Recently Deleted: Recover, Delete permanently (asks), 30-day retention ("Notes are deleted after 30 days."; erased at launch) | done, tested (a 40-day-old note erased, recent ones kept) |
| Quick Notes: the folder; `bramble-notes --quick-note` for a desktop shortcut (the Mac's fn-Q / hot corner), "Always resume to last Quick Note" | done, tested (new note each time; with resume, the last one) |
| Lock Note, Remove Lock, Close All Locked Notes; fingerprint (fprintd) / Windows Hello / Touch ID to open | done, tested (sweep) |
| Export To ▸ PDF (pages laid out like Print, on a worker), Markdown (one or many) | done, tested (`print::tests`, sweep) |
| Print… ⌘P: printer, copies (lp); no printers → the PDF opened | done, tested (sweep) |
| Import to Notes…: Mac libraries (NoteStore.sqlite / group.com.apple.notes), text, Markdown, HTML, RTF, Evernote .enex (its notes' dates), several files, folders dropped on the sidebar or list (their tree kept), into Imported Notes | done, tested (`import::tests`, scripted import) |
| Import Markdown… | done |
| File > Export Library… (the whole library as one .bnotes file) and Import to Notes… taking .bnotes back (its notes and folders merged in; ones already here left alone), both on a worker | done, tested (1186 notes out and into an empty library) |
| The library's one-time conversion to the native format: a notice with what was converted, or an alert if it stayed on the old library | done, tested (old-layout copy: 1233 notes, 30 folders, 282 attachments) |
| File > Export… (and the Share button's Export…): a sheet with a card per format, each with its Export button: Text; HTML (Collapsible Section: Expand all / Collapse all / As shown, as `<details>`); PDF (Orientation: Portrait / Landscape; Collapsible Section: Expand all / As shown); Markdown (additional data: YAML front matter with title, created, updated, folder path, tags). For the note, or all the list's notes (each its own file, in a folder named after it); written on a worker | done, tested (each format for one note and for a 36-note folder; nt-core `option_tests`) |
| Open in Pages | skipped: Apple's Pages (a follow-up could hand the note to Bramble Letters) |
| Share (Collaborate, Send Copy), Manage Shared Note, Show Note Activity ⌃⌘K, Show Highlights | skipped: iCloud collaboration |
| Edit > Record Audio…: the recording sheet (round record / stop button, time, level), Done puts it in the note at the caret as m4a (AAC, with ffmpeg when installed; else WAV); greyed in locked notes | done, tested (scripted runs record a tone, never the microphone: the note gets "Audio Recording.m4a", 0:01) |
| Audio attachments played in the note | notes-editor (rodio added for it) |

## Settings (Notes > Settings… ⌘,)
| | status |
|---|---|
| Sort notes by, New notes start with, Default account, Group notes by date, Automatically sort checked items, Default text size (7 stops) | done |
| Always resume to last Quick Note | done (with `--quick-note`) |
| Locked notes: Set / Change Password per account (login-style or separate, hint), the system's biometrics | done |
| Reset Password…: Apple's alert ("a new password for notes you lock from now on…"), then the Set Password sheet | done |
| Use dark backgrounds for note content (off: light paper in dark mode) | done (notes-editor) |
| Allow mention notifications | skipped: iCloud collaboration (kept in the file) |
| Enable the On My Mac account | skipped: every account here is local (kept in the file) |

## Sync (not on the Mac: iCloud's place)
| | status |
|---|---|
| Settings > Sync: WebDAV (Nextcloud, Koofr, NAS presets), address, user, password, folder; Test Connection, Sync Now, Replace Server… / Replace Notes Here… (asking first); last sync and outcome | done, tested (two libraries through a local WebDAV server: sent, received, an edit carried over) |
| Syncs at start, 5 s after a change, every minute; on a worker with its own Store; reloads what came in | done, tested |
| CalDAV | skipped: notes aren't calendar data; WebDAV is what ChordPro uses too |

## Help
| | status |
|---|---|
| Notes Help, Using Tags, Using Smart Folders | done, tested (sweep) |
| Send Notes Feedback | skipped: Apple's feedback service |

## Speed and stalls
| | status |
|---|---|
| Library opened on a worker (light notes, search warm-up, Recently Deleted's purge); first frame ~0.35–0.6 s | done, tested |
| Heavy work off the UI thread: thumbnails, attachments browser, PDF export / print pages, import reading | done |
| Watchdog (debug builds, NT_WATCHDOG=1): frames over 50 ms logged with their slowest parts (named: sidebar, list, editor, toolbar, dialogs, each menu command); a frame stuck over a second reported while it lasts, with the part it's in | done |
| Menu sweep, release build: all 53 commands run (Quit left out), no panic, no stuck frame; worst frames ~55–65 ms = the one frame the library arrives in (first sidebar counts, list and note layout) | tested |
| Found and fixed: the gallery built every card each frame (428 ms on 1230 notes → 18 ms, only cards in view); the store's first full note read (~0.3 s) moved from the first click to the loading worker | done, tested |
| Monkey sessions, release build (12 × 60 random actions on the user's library copy): no panics; the remaining stalls are the store's (first write after opening ~0.4 s, occasional 1–3 s waits in note() / create_note / save_note: commits and checkpoints), passed to notes-format | tested |

## Menus
Every command in Apple's menu bar (`cargo test -p nt-app menu_audit -- --nocapture`): 58 handled by the shell, 89 by the editor, 2 headings (greyed on the Mac too), 1 greyed (Photo Browser).
The editor's commands are covered in [PARITY-EDITOR.md](PARITY-EDITOR.md).

| Menu item | handled by | status |
|---|---|---|
| Notes > About Notes | shell | done, tested (sweep) |
| Notes > Settings… | shell | done, tested (sweep) |
| Notes > Close All Locked Notes | shell | done, tested (sweep) |
| Notes > Quit Notes | shell | done |
| File > New Note | shell | done, tested (sweep) |
| File > New Folder | shell | done, tested (sweep) |
| File > New Smart Folder | shell | done, tested (sweep) |
| File > More > New Smart Folder with Tag Selection | shell | done, tested (sweep) |
| File > More > Convert Folder to Smart Folder | shell | done, tested (sweep) |
| File > Close | shell | done, tested (sweep) |
| File > Close All | shell | done, tested (sweep) |
| File > Import to Notes… | shell | done, tested (sweep) |
| File > Import Markdown... | shell | done, tested (sweep) |
| File > Export To > PDF | shell | done, tested (sweep) |
| File > Export To > Markdown | shell | done, tested (sweep) |
| File > Pin Note | shell | done, tested (sweep) |
| File > Lock Note | shell | done, tested (sweep) |
| File > Duplicate Note | shell | done, tested (sweep) |
| File > Print… | shell | done, tested (sweep) |
| Edit > Delete | shell | done, tested (sweep) |
| Edit > Rename | shell | done, tested (sweep) |
| Edit > Attach File… | shell | done, tested (sweep) |
| Edit > Record Audio… | shell | done, tested |
| Edit > Find > Note List Search… | shell | done, tested (sweep) |
| Format > Alignment > Writing Direction > Paragraph | — | heading (greyed on the Mac) |
| Format > Alignment > Writing Direction > Selection | — | heading (greyed on the Mac) |
| View > as List | shell | done, tested (sweep) |
| View > as Gallery | shell | done, tested (sweep) |
| View > Recent Notes > Previous Note | shell | done, tested (sweep) |
| View > Recent Notes > Next Note | shell | done, tested (sweep) |
| View > Recent Notes > Clear Menu | shell | done, tested (sweep) |
| View > Sort By > Default (Date Edited) | shell | done, tested (sweep) |
| View > Sort By > Date Edited | shell | done, tested (sweep) |
| View > Sort By > Date Created | shell | done, tested (sweep) |
| View > Sort By > Title | shell | done, tested (sweep) |
| View > Sort By > Newest First | shell | done, tested (sweep) |
| View > Sort By > Oldest First | shell | done, tested (sweep) |
| View > Group By Date > Default (On) | shell | done, tested (sweep) |
| View > Group By Date > On | shell | done, tested (sweep) |
| View > Group By Date > Off | shell | done, tested (sweep) |
| View > Hide Sidebar | shell | done, tested (sweep) |
| View > Hide Note Count | shell | done, tested (sweep) |
| View > Show Attachments Browser | shell | done, tested (sweep) |
| View > Show in Note | shell | done, tested (sweep) |
| View > Zoom In | shell | done, tested (sweep) |
| View > Zoom Out | shell | done, tested (sweep) |
| View > Actual Size | shell | done, tested (sweep) |
| View > Hide Toolbar | shell | done, tested (sweep) |
| View > Customize Toolbar… | shell | done, tested (sweep) |
| View > Enter Full Screen | shell | done, tested (sweep) |
| Window > Minimize | shell | done, tested (sweep) |
| Window > Zoom | shell | done, tested (sweep) |
| Window > Open Note in New Window | shell | done, tested |
| Window > Float on Top | shell | done, tested (added: the Mac shows it in a note window's Window menu) |
| Window > Notes | shell | done, tested (sweep) |
| Window > Photo Browser | — | skipped: Apple Photos |
| Window > Notes Settings | shell | done, tested (sweep) |
| Help > Notes Help | shell | done, tested (sweep) |
| Help > Using Tags | shell | done, tested (sweep) |
| Help > Using Smart Folders | shell | done, tested (sweep) |

