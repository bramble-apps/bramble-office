# Suite to-do list (Keydeck, Values, Letters)

Last updated 2026-10-20. Mac research rounds live in the research folder (testdata/research) (out-N29 is done; N30 is queued with Keydeck).

## Waiting on other sessions
- [ ] **Keydeck to review and commit two undo fixes** (uncommitted in keynote-linux, made 2026-10-21 for Values' Ctrl+Z bug):
  - `kn-model/src/tablefmt.rs` `set_cells_border`: existing stroke layers are now recorded before they change, so undoing a border works. Before, borders never undid and stacked up.
  - `kn-model/src/edit.rs`: no 500-step cap; whole-file snapshots past the latest 20 are kept compressed (`whole_zip`) instead of dropped, so the whole session undoes.
  - Tests: `the_whole_session_undoes` (kn-model) and numbers-linux `crates/nm-core/tests/undo_formatting.rs`.
  - Until these are committed, `undo_formatting.rs` fails in the shared repo github.com/bramble-apps/bramble-office ("undo back to step 27 isn't what it was"). Once committed: run `keynote-linux/tools/suite/export.sh ~/CascadeProjects/bramble-office` and push (ask the user before pushing).
- [ ] **Letters to commit**: the Chart Data editor, chart picker and chartedit changes in pages-linux (`chartedit.rs`, `toolbar.rs`, `objinsp.rs`, `menus.rs`, `canvas.rs`). The kn-ui parts are already committed by Keydeck (d4a9dfa, a1afc18).
- [ ] **Mac round N30 is in** (`out-N30`, notes in `SUMMARY-N30.txt`; Pages still pending because Pages is hung):
  - Chart popover (Keynote and Numbers): compare the picker tiles with the screens. The layout already matches: 14 / 8 / 4 tiles, 3 columns, 6 style pages, sliders on Interactive. Send picker changes to Keydeck for review.
  - Pivots: C4 / C4b / C4c (Summarize), C5 / C5b (Show as), C6 (totals hidden). Map agg_type / show_as_type from them.
  - Numbers' value headers name the summary ("Sales (Max.)", "Sales (Unique)"), but not Show as (still "Sales (Sum)"). Values' header names need checking against these.

## Values (numbers-linux)
- [ ] **Sort rules**: wire the Sort pane to `nm_core::organize::sort_rules` / `set_sort_rules`, which are written and tested:
  - read the rules when a document opens;
  - save them when they change;
  - sort as soon as a rule is added or changed, as the Mac does (N29-I1).
- [ ] **Saved filter rules**: read and write `TST.FilterSetArchive`.
  - Predicate types known so far: 5–10 are number comparisons (equal, not equal, greater, greater or equal, less, less or equal); 36 is text "is".
  - The rules' column offsets are relative to something not yet worked out. Needs a Mac capture with one filter on each of columns A, B and C, plus one rule of each kind.
- [ ] **Saved categories**: check that Values' own categories save in Numbers' format. Reading Mac categories works; compare against N29-I2.
- [ ] **Pivot tables in Numbers' format**:
  - Values' own pivots are plain tables for now, with their fields kept only while the document is open;
  - Mac pivots are read and shown with their totals, but editing their fields doesn't save yet;
  - after N30 C4/C5: map agg_type / show_as_type and write `TST.PivotOwnerArchive`;
  - the Pivot Options pane has no Filter tab.
- [ ] **Smart Web and Email Links in table cells**: cells are plain text today, so this needs rich-text cells. It works in text boxes and shapes.
- [ ] **Pen tool**: an open path should become a line with Line Styles; a closed path, a shape (N29-F2).
- [ ] **Movie and Audio panes** (N29-B1/B2/B3):
  - File Info, rewind / fast-forward, Description, the Repeat pop-up;
  - Audio tabs Audio / Arrange;
  - recorded audio shows "Edit Recording" instead of Trim.
- [ ] **Print headers and footers**: Insert > Page Number and Page Count, "Page n of count" and a date footer (N29-D). These are the last greyed-out Insert items.
- [ ] **Placeholders and templates** (N29-E1–E5):
  - Define as Text / Media Placeholder (Text Placeholder needs the text selected);
  - template file format, default text box, table style, Language & Region / Change File Type.
  - These probably touch kn-*, so tell Keydeck first.
- [ ] **Web video, image gallery with caption, equation**: compare with N29-B4–B6.
- [ ] **Charts**:
  - Edit Data References doesn't follow references when the linked cells move: Numbers rewrites them; Values keeps the old range.
  - Check on the Mac that a chart linked by Values keeps updating in Numbers.
- [x] **Content Hub and shape library** (decided 2026-10-20; done 2026-10-30: shapes from Phosphor in kn-model/shapelib, Content Hub from Wikimedia Commons' Featured pictures in kn-ui/contenthub, in all three apps):
  - **Content Hub:** fill it from free and openly licensed online libraries. It doesn't need to match the Mac's 814 items one for one; what matters is that it's nicely laid out and curated, with categories like the Mac's (N29-G1).
  - **Shapes:** packaged with the apps.
  - **Styling:** applied as filters on the shipped art (stroke, recolouring a PNG or vector), not separate artwork per style.
  - **Real images** (the Mac's "architecture" pictures, the charts and so on): just noted, nothing planned for now.
  - **Check licences before bundling.** Unsplash's licence doesn't allow compiling its photos into a similar or competing service. Freepik's free licence needs attribution and doesn't allow redistributing files. Bundling either inside the apps may not be allowed.
  - **Likely safer sources:** Openverse / Wikimedia Commons (CC0 and public domain); icon and shape sets under MIT / ISC / CC BY, such as Tabler, Lucide, Phosphor, OpenMoji and Twemoji. Keep the attributions they require.
- [ ] Re-run the `list_greyed_commands` test at the end. Today it lists only Insert > Page Number / Page Count and the two placeholder items.

## Shared (kn-* crates; Keydeck reviews and commits)
- [ ] Chart text is smaller than on the Mac with the fallback font. Compare at the same zoom with a real Avenir and send Keydeck measured cap heights if it still differs.
- [ ] Chart picker tiles are my own drawings until the N30 screenshots arrive.

## Done today (for reference)
- Text links, Sheet links, link popover and badge, Smart Web and Email Links in text.
- A shared Chart Data editor that follows the chart type (Plot Rows / Columns as Series, pie, scatter, bubble).
- Charts linked to table cells, and Edit Data References.
- A shared chart picker with previews, in all three apps.
- Charts: log-axis bug, legend and value labels (Keydeck), and placement of new charts in Values.
- Pivot tables: Numbers' layout, all Summarize and Show as options, Hide Grand Total, Swap / Clear fields, Last refreshed; Mac pivots read and shown with their totals.
