# Values vs Keydeck: feature audit (2026-10-20)

How each Keydeck (kn-app) feature that also belongs in Numbers stands in Values (nm-app).
Keynote-only features (slides, builds, transitions, play, presenter, motion paths) are left
out. Status: **missing**, **partial**, **done**.

## Charts
- **partial**: insert from selected cells, chart type, legend, title, Axis / Series tabs.
- **missing**: Insert > Chart with nothing selected (Numbers adds a chart with sample data).
- **missing**: Chart pane's "Edit Chart Data" button. The data editor (chartedit.rs) exists, but
  only the Chart > Edit Chart Data… menu reaches it.
- **missing** (in Keydeck too, so shared work): chart styles and colours, chart font and size,
  background / border / corners, gap between bars, value-label formats, axis scale (min / max /
  steps, log), number format, number of decimals. Numbers-only: Edit Data References
  (re-pick the table cells), live link to the table.
- **missing**: a new chart's position. It lands under the table, partly off screen.

## Media (Insert menu / Media button)
- **partial**: images (Media button picks an image file).
- **missing**: Keydeck's media browser (photos, music, movies in the home folders); Movie and
  Audio (kn_ui::media Player, probe, prepare); Record Audio; Web Video (kn_ui::media::web);
  Live Video; Image Gallery; Equation… These are listed in Values' Insert menu but do nothing.
- **missing**: Image Description, Replace image, Instant Alpha / Remove Background.

## Shapes and lines
- **partial**: basic shapes, fill / border / shadow / reflection / opacity, Make Editable points,
  boolean shape operations (Unite / Intersect / Subtract / Exclude), mask.
- **missing**: Keydeck's shape library (Shape > Star, Polygon, Callout, Quote Bubble, Arrow…
  and its categories), Save to My Shapes / My Shapes, Draw With Pen, connection lines
  (straight / curved / angled), Set as Default appearance.
- **partial**: style presets (Keydeck shows the theme's 6 shape styles; Values fewer).

## Text
- **missing**: Line Break / Column Break / Page Break insertion, Transform Text (upper / lower
  / title case), Increase / Decrease Indent Level menus, Text Replacement / Substitutions
  window, Start / Stop Speaking, hyperlinks on text (Add Link).

## Tables
- **missing**: Table > Header Rows / Header Columns / Footer Rows submenus (0–5), Formula > Sum /
  Average / Minimum / Maximum / Count / Product, Resize Row / Column to Fit Content, Distribute
  Rows / Columns Evenly menu entries, Autofill Up / Down / Left / Right menu entries.
- **done**: autofill drag, Organize (sort / filter / categories), conditional highlighting,
  cell Text inspector, multi-line cells.

## Document and windows
- **missing**: Object List, Show Warnings, Browse All Versions, Change Password, Replace
  Fonts…, Zoom to Selection / Fit Content, Hide / Show Comments, Language & Region.
- **missing**: Export To > PowerPoint is Keynote's; Numbers' Export To > Excel / CSV / PDF are done.

## Order of work
1. Charts: Edit Chart Data button, chart with sample data, chart placement, then the shared
   chart styling (with Keydeck).
2. Media: movies, audio, web video, image gallery, equations through kn_ui::media.
3. Shapes: shape library, connection lines, Draw With Pen, My Shapes.
4. Text and table menus listed above.
5. Document windows (Object List, Warnings, Versions, Replace Fonts).
