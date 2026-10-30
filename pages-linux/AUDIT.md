# Letters vs Keydeck (and Pages 15.4) — feature audit, 2026-10-20

Asked for by the user ("many features have only been partially implemented on letters … compared
to keydeck, shapes … attachment menu, audit all the missing features"). Method: the commands each
app's `menus::run` handles, the Pages menus Letters lists but greys (`cargo test -p pg-app audit
-- --nocapture`), the inspector panes' sections and controls, the toolbar popovers, and the canvas'
object editing (`enum Drag`), against Keydeck's kn-app and the Mac captures in
the Mac research folder (`pages-research/features`). Slideshow / slide / presenter items are Keynote-only and left out.

Legend: [ ] missing, [~] partial, [x] done since the audit.

## Shapes and lines (canvas)
- [x] Green control handles: arrow heads, star points / inner radius, polygon sides, rounded
      rectangle corner radius, callout tail (Keydeck `Drag::Control`)
- [x] Straight lines show two end handles instead of a selection box; drag an end (`Drag::LineEnd`)
- [x] Connection lines (straight / curved / right angle) that follow the objects they join;
      Shapes and Lines > Toggle Line Type (dragging a free line's end onto an object to join it: not yet)
- [x] Shapes and Lines > Break Apart, Reset Text and Object Handles
- [x] Shape > Polygon, Mask with Shape > Polygon
- [x] Shape popover: Text box tile, Line with arrowhead, filled icons, My Shapes
      (Format > Advanced > Save to My Shapes)
- [x] Guides: drag from the ruler (horizontal) or right-click > Add Horizontal / Vertical Guide;
      move, drag off to remove, objects snap; Hide Guides, Hide Page Template Guides, Clear All
      Guides on Page. Stored like Pages (section guide map per page; template guide storage)

## Media (toolbar Media ▾ = Pages: Choose Photo or Video, Choose Music, Add Image Gallery,
## Add Web Video, Record Audio, Choose File)
- [x] All six items (shared kn_ui::media); files dropped on the canvas too
- [x] Movies / audio as objects: insert, save, play in place; Movie / Audio pane (volume, repeat, trim, poster frame)
- [x] Image gallery: insert, ‹ › on the canvas, Gallery pane (caption mode, images + / −, descriptions).
      Fixed: the Pages schema lacked TSD.ImageArchive's gallery_info / web_video_info extensions,
      so Add Image Gallery and Add Web Video crashed. Not yet: drawing the gallery's caption
- [x] Web video (YouTube / Vimeo link)
- [x] Record Audio
- [x] Choose File (any file as an attachment object)

## Charts
- [x] Edit Chart Data button (pane, under the selected chart, double-click)
- [x] Format > Chart > Chart Type ▸ (2D / 3D types), Show Title / Legend / Borders, axis names
      and labels, series names and value labels (menu toggles)
- [x] Toolbar Chart popover: 2D / 3D / Interactive tabs with every type (interactive charts show
      their first data set; no slider to step through the sets yet)
- [~] Chart / Axis / Series panes are Keydeck's (basic): Pages' richer options (chart style
      presets, fonts, gaps, axis scale / format / ticks, value label format, trendlines) are
      missing in all three apps. Keydeck is building them as shared work: kn-model chartstyle.rs
      (chart_prop / set_chart_prop per ChartPart), kn-render drawing, then kn_ui::chartpanes,
      which Letters wires in place of its own panes when Keydeck says each phase is committed.
      Presets, trendlines and error bars wait for Mac round N29

## Tables
- [x] Table > Select ▸ (Row, Column, First / Last Value in Row / Column, Extend to …)
- [x] Table > Autofill Cells ▸ (Autofill, Right / Left / Down / Up, from Row Above / Column Before)
- [x] Table > Resize Row / Column to Fit Content (the tab menus already fit)
- [x] Table > Repeat Header Rows on Each Page
- [x] Table > Formula > New Formula, Text > Auto-Align Table Cell
- [x] Conditional Highlighting… (Format menu and Cell pane; add a rule, clear all rules)

## Inspector
- [x] Image pane: Replace (picture), Image Description (were already there)
- [x] Image gallery pane
- [x] Web Video pane (title, author, link, Replace)

## Menus
- [x] File > Open Recent (submenu)
