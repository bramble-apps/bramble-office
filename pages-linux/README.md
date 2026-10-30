# Letters

A word processor and page-layout app for Linux that opens and saves real Apple Pages (`.pages`) files.
It is built on Keydeck's iWork crates in `../keynote-linux` (`kn-format` file IO, `kn-model` styles,
text and objects, `kn-render` drawing, `kn-ui` look) and reuses Values' canvas, inspectors and
toolbar. The research it follows is in `../pages-research-src/pages-research` (Mac rounds P1…).

    ./run.sh [file.pages | file.docx | file.rtf | file.txt | file.md]

`packaging/install-local.sh` installs it for this user (app menu entry, icon, file types).

## What works
The full list, with what has been tested, is in [FEATURES.md](FEATURES.md). In short:
- **Files:** `.pages` (single file and package, password-protected too), lossless round-trip; new
  documents from Blank / Blank Layout / the template chooser; import of Word, RTF, HTML, Markdown and
  plain text; export to PDF, Word, RTF, EPUB, HTML, plain text and page pictures; print.
- **Text:** the body flows across pages with columns, page / column / section breaks, keep-with-next,
  widow and orphan control, drop caps, lists, tabs, footnotes, a table of contents, headers and
  footers with page numbers, sections with their own numbering and backgrounds.
- **Formatting:** paragraph and character styles (update, revert, create), fonts, sizes, bold /
  italic / underline / strikethrough, baseline, capitalisation, spacing, colours, borders, indents on
  the ruler, alignment, line spacing.
- **Objects:** text boxes, shapes, lines, pictures (masks, adjustments), tables with formulas,
  charts with their data editor, equations; move, resize, rotate, flip, lock, group, align, wrap,
  Stay on Page / Move with Text, Make Editable and shape unite / intersect / subtract / exclude.
- **Review:** comments and highlights, track changes with accept / reject, links and bookmarks.
- **View:** page thumbnails and table of contents sidebar, zoom (fit, two pages, pinch), Screen View
  (text reflows to the window), find and replace, word count, invisibles, Go To page.

## Layout
- `crates/pg-core`: Pages specifics. It loads the Pages schema (TP.*), lays out the body text into
  pages (`flow.rs`), draws pages (`render.rs`), edits text, styles, objects, fields, comments and
  changes, sweeps unreachable objects on save (`gc.rs`), and imports / exports other formats.
- `crates/pg-app`: the `letters` window (egui): toolbar, canvas with tiled pages, inspectors, menus.

## Tests
- `cargo test -p pg-core`: every research sample opens and re-saves losslessly with no orphaned
  objects; edit tests write `target/pg-edits/*.pages`, which go to the Mac to be opened in Pages.
- GUI checks with no human present: the app screenshots itself and quits.

      CS_SNAP=out.png [CS_PAGE=n] [CS_ZOOM=z] [CS_CMD=menu item,…] [CS_SAVE=out.pages] [CS_DUMP=1]
      [CS_LINES=1] [CS_SCRIPT="click x y; dclick x y; tclick x y; sclick x y; rclick x y;
      drag x1 y1 x2 y2; cdrag x1 y1 x2 y2; type text; key ctrl+B; scroll x y dx dy n; pinch x y f n;
      menu Insert item; image path; wait"] [CS_PERF=1] letters file.pages

  Script coordinates are window points (screenshot pixels / 2 on a 2× display); `\s` in `type` is a
  space. Headless conversion: `letters --pdf|--docx|--rtf|--txt|--epub|--html|--png in.pages out`.
- `crates/pg-core/examples`: `snap` (render pages to PNG), `body` (dump the body, styles, runs, setup,
  objects), `cells` (table contents), `pw` (open with a password).

## Licensing note
`crates/pg-core/data/Blank.pages`, `BlankLayout.pages` and `data/donors/*.pages` were made in Pages and
hold the document structure Letters copies to create new documents and objects. Every image and preview
in them has been replaced by an original one (`keynote-linux/crates/kn-render/examples/clean_donor.rs`).
The donors used only by tests, and the research samples, are kept on this computer and never committed.
