# Values

A spreadsheet app for Linux that opens and saves real Apple Numbers (`.numbers`) files. It is built on
Keydeck's iWork crates in `../keynote-linux` (`kn-format` file IO, `kn-model` tables and formulas,
`kn-render` drawing). The research it follows is in `../numbers-research-src/` (Mac rounds N1–N5).

    ./run.sh [file.numbers | file.csv]

## What works (2026-10-19)
- **Open and save:** `.numbers` (lossless round-trip; password-protected files too). It also opens CSV/TSV
  and Excel `.xlsx` as new documents, and exports PDF (one page per sheet) and CSV.
- **Sheets:** add (with a table), rename, duplicate, delete, reorder (drag the tabs).
- **Tables:** insert and delete, move (corner handle), resize columns/rows (reference bars), add/delete
  rows and columns, headers/footers, merge, sort, autofill (drag the handle), table styles, borders, fills,
  data formats, text style/alignment, conditional highlighting, title on/off.
- **Cells:** type values or formulas. Click cells to insert references while typing a formula, and
  functions autocomplete. Formulas that refer to other tables or sheets show and recalculate.
  Checkboxes and star ratings respond to clicks. Comments can be added. Tables can be large (cells in
  several tiles, as Numbers stores tables over 256 rows).
- **Shown like Numbers:** hidden/filtered rows, categories (group rows), frozen header rows and columns, comments, charts,
  shapes, text boxes (insert and edit), pivot tables (stored values).
- **Other:** Find (Ctrl+F), quick calculations (SUM/AVG/MIN/MAX/COUNT) of a selection, undo/redo,
  copy/cut/paste (tab-separated), zoom, Open Recent.
- **Not yet:** inserting charts/shapes/images, filters and categories editing, print dialog.

## Layout
- `crates/nm-core`: Numbers specifics. It loads the Numbers schema (TN.*), reads sheets and hidden/filtered
  rows, and edits sheets (add, rename, move, delete), tables (insert via the Blank template's table,
  delete) and drawables. It also imports CSV/TSV.
- `crates/nm-app`: the `values` window (egui). It has the toolbar, sheet tabs and a tiled sheet canvas
  drawn by Keydeck's renderer. On top come reference bars, cell selection and editing, formulas with
  click-to-insert references, autofill, resizing, the Format inspector and menus.
- `data/functions.tsv`: TSCE function ids read from Numbers' own file (corrects the N1 table).

## Tests
- `cargo test -p nm-core`: every research sample opens and re-saves losslessly. The edit tests write
  `target/n-edits/*.numbers`, and those files go to the Mac to be opened in Numbers 15.4.
- GUI checks with no human present: the app screenshots itself and quits.

      NM_SNAP=out.png [NM_CELL=r,c[,r2,c2]] [NM_EDIT=text] [NM_ZOOM=z] [NM_CMD=insert-table,add-sheet]
      [NM_SCRIPT="click x y; dclick x y; drag x1 y1 x2 y2; type text; key Enter; key ctrl+Z; scroll x y dx dy n; wait"]
      [NM_PERF=1] values file.numbers

  Script coordinates are window points (screenshot pixels / 2 on a 2× display).
  Headless CSV export: `values --csv in.numbers out-folder`.

## Licensing note
`crates/nm-core/data/*.numbers` were made in Numbers and hold the document structure Values copies to create
new documents, sheets, tables, charts and shapes. Every image and preview in them has been replaced by an
original one (`keynote-linux/crates/kn-render/examples/clean_donor.rs`). The test files in `testdata/` are
kept on this computer and never committed.
