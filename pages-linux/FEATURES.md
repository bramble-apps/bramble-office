# Letters — feature list

Letters is a word processor and page-layout app for Linux that opens and saves Apple Pages
(`.pages`, Pages 15.4) files. It is built on the suite's iWork crates (`../keynote-linux`: kn-format, kn-model,
kn-render, kn-ui), and its look comes from Keydeck and Values. The research it follows is in
`../pages-research-src/pages-research` (the Mac's Pages 15.4 notes, samples, screenshots).

`[x]` = done and tested · `[~]` = done, tested only in part (see the note) · `[ ]` = to do.
"core" = a pg-core test (`cargo test -p pg-core`), "GUI" = a CS_SCRIPT run checked by its saved
file and a screenshot, "Mac" = opened in Pages 15.4 on the Mac (round P1).

## 1. Files
- [x] Open `.pages` (single file) and package `.pages` folders (Index.zip inside) — core (E13, sample 53)
- [x] Lossless save (every research sample opens and re-saves byte-identical objects) — core; saves leave no orphans, unused data, unused style variations or declared references to missing objects (core on every written file, Mac P1 static checks)
- [x] New document from the Blank (word processing) and Blank Layout (page layout) templates — core, GUI
- [x] Template chooser on New (Basic, Blank, Blank Landscape, Note Taking, Blank Layout, Essay) — GUI
- [x] Save / Save As / Duplicate / Rename / Revert (Browse All Versions, Restore), Open Recent — GUI (Save As goes through the system file dialog, which isn't scripted)
- [x] Password-protected documents (open and Set Password…) — core (E13), GUI (set in the dialog, saved, reopened with the password prompt)
- [x] Export to PDF, Plain Text, Rich Text (RTF), Word (.docx), EPUB, HTML, Images (PNG per page) — core; .docx checked in LibreOffice
- [x] Import Plain Text, RTF, .docx, HTML and Markdown as a new document — core (E7)
- [x] Headless conversion: `letters --pdf|--txt|--docx|--rtf|--epub|--html|--png in.pages out` — run on samples
- [x] Print (to PDF through the system's print command) — GUI

## 2. Text editing (body text flow)
- [x] Caret, typing, Backspace/Delete, Return (paragraph), Shift+Return (line break U+2028) — GUI
- [x] Mouse selection (click, drag, double-click word, triple-click paragraph, Shift+click) — GUI
- [x] Keyboard navigation: arrows, Home/End, Ctrl+arrows (word), Page Up/Down, Ctrl+Home/End, Shift to select — GUI
- [x] Cut / Copy / Paste (plain text and styled within the app), Paste and Match Style — GUI
- [x] Undo / Redo for every edit — core (undo back to the start after fuzzed edits), GUI
- [x] Select All, Deselect All — GUI
- [x] Insert Page Break (Ctrl+Enter, U+0005), Column Break (U+000C), Section Break (U+0004), Line Break — core, GUI
- [x] Tab characters — GUI
- [x] Smart quotes, dashes and text substitutions; auto-capitalisation; list detection ("* ", "1. ") — GUI
- [x] Spell checking (red underline) + suggestions on right click, Learn Spelling, Ignore Spelling — GUI
- [x] Find & Replace bar (Find Next/Previous, Replace, Replace All, match case, whole words) — GUI
- [x] Word count pill (words / characters / characters without spaces / paragraphs / pages) — GUI
- [x] Show Invisibles (¶, ·, →, page/section break marks) — GUI

## 3. Character formatting
- [x] Font family, typeface, size (inspector + Fonts window) — GUI
- [x] Bold, Italic, Underline, Strikethrough (Ctrl+B/I/U) — GUI
- [x] Bigger / Smaller (Ctrl+= / Ctrl+-) — GUI
- [x] Text colour (inspector, Colors window), text background — GUI
- [x] Superscript / subscript, baseline shift, character spacing (tighten/loosen), ligatures — GUI (enum values checked against the schema by a core test)
- [x] Capitalisation: All Caps, Small Caps, Title Case, Start Case — GUI
- [x] Character styles (None, Emphasis, Strikethrough, Underline … from the stylesheet) — GUI
- [x] Copy Style / Paste Style — GUI

## 4. Paragraph formatting
- [x] Paragraph styles popover (check mark on the current style), "Body*" when overridden, Update / Revert, create new style — GUI
- [x] Alignment left / centre / right / justify — GUI
- [x] Line spacing (Lines / At Least / Exactly / Between), space before / after — core, GUI
- [x] Indents (first line, left, right) on the ruler and in the inspector — GUI
- [x] Bullets & lists (None, Bullet, Lettered, Numbered, Harvard, Dash, Note Taking …), list levels (Tab / Shift+Tab) — core, GUI
- [x] Drop cap (spans its lines from the first line's cap height) — core, GUI
- [x] Paragraph borders and background colour — GUI
- [x] Pagination: keep lines together, keep with next, start on new page, widow & orphan control — core, GUI
- [x] Columns (count and gutter) — core (E9), GUI
- [x] Following paragraph style (Heading → Body on Return) — GUI

## 5. Pages and layout
- [x] Word-processing layout: the body flows across pages; margins; page breaks; column breaks; columns — core
- [x] Page-layout documents: free pages, Add Page, Delete Page (menu and thumbnails), each page its own objects; undo — core (E4), GUI
- [x] Convert to Page Layout / Convert to Word Processing (Document Body switch) — core (E10), GUI
- [x] Headers and footers, editing in place, hide on first page — core, GUI
- [x] Header / footer text: click, double / triple click, drag to select; character and paragraph formatting; a field's own alignment across the text width (natural = its left / centre / right place) — core, GUI
- [x] Insert Page Number button below a header / footer field once clicked into (hidden when it already has one), Insert > Page Number and the right-click menu: "1", "Page 1", "1 of N", "Page 1 of N"; number style 1 / i / I / a / A — core, GUI
- [x] Page number and page count fields, date & time field — core, GUI
- [x] Footnotes (insert, numbered, shown at the page bottom) — core, GUI
- [x] Sections: section break, per-section background, page numbering start — core (E9), GUI
- [x] Table of contents (inserted, from heading styles in the TOC entry style, page numbers, clickable in the sidebar) — core, GUI
- [x] Facing pages (two-page spreads, page 1 on the right) — GUI
- [x] Inline objects (on their line among the text, or a block of their own) and floating objects (Stay on Page) with text wrap — core, GUI

## 6. Document inspector
- [x] Document tab: paper size, orientation, header/footer on and distances, Document Body switch, margins, facing pages, hyphenation, ligatures — GUI
- [x] Section tab: background fill, hide header/footer on first page, match previous, page numbering format and start — GUI (hide + start at), core (background)
- [x] Bookmarks tab: list, add bookmark, go to; Add Link > Bookmark — core (E8), GUI

## 7. Objects (reused from Values / Keydeck)
- [x] Insert text box, shapes, lines, images (from a file), tables, charts — core (E2), GUI
- [x] Select, move, resize, rotate (Ctrl-drag a handle) objects; snapping guides with the position — GUI
- [x] Format inspector Style / Text / Arrange tabs for shapes, text boxes and images — GUI
- [x] Image tab: mask (Edit Mask crops; a masked picture resizes with its mask), adjust image, description — GUI
- [x] Tables: cell editing, formulas, add rows & columns, merge, sort, cell context menu — GUI
- [x] Tables, Keydeck's chrome: column / row tabs (select, drag-select, move rows, tab menus), tab-line and border drag resizing, corner button (whole table / move), add rows / columns buttons (drag or stepper), drag-select cells, autofill; resizing by the handles scales rows and columns — GUI
- [x] Table cells edited in place, multi-line: Return enters, Shift+Return a new line, Tab moves on, formula bar for "=" — GUI
- [x] Charts: edit chart data — GUI
- [x] Arrange: front/back (stacking kept in the layout), align, flip, lock, group/ungroup — GUI
- [x] Object placement Stay on Page / Move with Text; Text Wrap — core, GUI
- [x] Copy / paste / duplicate / delete objects (each paste steps further) — GUI
- [x] Shapes: Make Editable (drag points, the frame follows) and unite / intersect / subtract / exclude — GUI
- [x] Equations: LaTeX typed in, inserted as an inline picture that keeps its source (typeset by pdflatex when installed, otherwise drawn by Letters); double-click to edit — GUI

## 8. Annotations
- [x] Comments: add, show, edit, delete, comments pane, next / previous — core (E5), GUI
- [x] Highlights (drawn in the author colour; hidden with Hide Comments) — GUI
- [x] Track Changes: on/off, insertions and deletions shown in author colour, Accept / Reject (one / all), annotation bar — core (E5/E6), GUI
- [x] Author name and colour (Settings, View > Author Color) — GUI
- [x] Hyperlinks: Add Link (webpage, email, phone, bookmark), Ctrl+click opens, Remove Link (and its style) — core (E8), GUI

## 9. Window and view (Keydeck / Values look, orange accent)
- [x] Own window frame with menus in the title bar, toolbar with pills like Pages — GUI
- [x] Sidebar: Page Thumbnails / Table of Contents / Document Only — GUI
- [x] Zoom: One Page, Two Pages, Zoom In/Out, Fit Width, Fit Page, Actual Size, pinch — GUI
- [x] Ruler (horizontal, margins shaded, indent markers to drag, in the Settings units) — GUI
- [x] Screen View (the body reflows to the window; breaks become space; no headers/footers) — GUI
- [x] Go To page — GUI
- [x] Customize Toolbar (drag items in), Hide Toolbar — GUI
- [x] Full screen; Colors window (applies to the selection), Show Fonts — GUI
- [x] Settings window: General, Rulers (units shown everywhere), Auto-Correction — GUI
- [x] Keyboard Shortcuts window, About window — GUI
- [x] Context menus (body text with spelling, object, table cell, page thumbnail) — GUI
- [x] 120 Hz smooth scrolling (page strips cached per zoom, glyphs cached as bitmaps) — release build scrolling a 30-page document: p50 3.0 ms, p95 8.0 ms a frame (CS_PERF)

## 10. Test hooks and quality
- [x] `CS_SNAP=out.png [CS_PAGE=n] [CS_ZOOM=z] [CS_CMD=…] [CS_SAVE=…] [CS_DUMP=1] [CS_LINES=1] [CS_SCRIPT="click x y; cdrag …; type …; key …; menu …; image …"] letters file.pages`
- [x] Round-trip tests for every sample; key_validate.py on every written file (only the issues the source samples already have)
- [x] Fuzzed edits (random insert/delete/format on every word-processing sample) keep files valid, save clean and undo back — core
- [x] Launcher (packaging/letters.desktop, install-local.sh, icon), `run.sh`, README
