# Calculator engine — API and UI spec

A feature-for-feature remake of Apple's iOS Calculator. One Rust engine (`crates/calc-core`)
decides the maths, the display text, **where every key goes at every window size**, the
themes' colours, and the icons. Two front ends draw it:

- `crates/calc-app` — desktop, pure Rust (egui/eframe 0.37), calls `calc_core::session::Session` and `calc_core::api::call` directly.
- `../calculator_mobile` — Android, Flutter UI over `libcalc_engine.so` (`crates/calc-ffi`): `cc_init(dir)`, `cc_call(json) -> json`, `cc_free(ptr)`.

Reference screenshots (the user's): `docs/reference/*.png|webp`
(1 = iOS 26 App Store shots, 2 = iPad iOS 26, 3 = iOS 18 iPhone + iPad, 4 = iOS 18 landscape,
5 = the calculator through the years 1984/2001/2007/2019/2024 (the themes), 6 = convert mode +
unit sheet, 7 = history panel + edit, 8 = iOS 18 mode menu, 9 = mode menu with Convert switch +
unit list, 10/12 = iOS 26 portrait scientific, 11 = iOS 26 mode menu, convert, Units sheet).

## Requests (`api::call(&mut Session, &json)`)

All return JSON. "→ frame" means the full screen state (below).

| cmd | fields | returns |
|---|---|---|
| `frame` | `w`,`h` (logical px), `insets{top,bottom,left,right}` | frame — call on every resize/rotation |
| `get` | | frame |
| `press` | `key` (a key id from `layout.keys[].id`) | frame |
| `swipe` | | frame (swipe left/right on the number deletes the last digit) |
| `char` | `ch` (typed character) | frame |
| `mode` | `mode`: `"Basic"`/`"Scientific"`/`"Notes"` | frame |
| `convert` | `on`: bool | frame |
| `theme` | `id` | frame |
| `style` | `style`: `"Comma"`/`"Dot"`/`"Space"` | frame |
| `sidebar` | `open`: bool (tablet history sidebar) | frame |
| `settings` | `haptics`?, `sounds`? | frame |
| `themes` | | `[Theme]` (for the theme picker; draw a mini preview from its colours) |
| `styles` | | `[{style, example}]` |
| `units` | | `[{id,name,units:[{id,name,symbol}]}]` 15 categories, iOS order |
| `unit_search` | `q` | `[{cat, unit}]` |
| `set_unit` | `top` bool, `cat`, `unit` | frame |
| `swap` | | frame (⇅ button) |
| `rates_url` / `rates_stale` / `rates_set{json}` | | url string / bool / bool. The app downloads `rates_url` (HTTP GET) when the converter opens and rates are stale, then passes the body to `rates_set`. |
| `history` | `utc_offset` (secs) | `[{title, rows:[{id, expr:[Seg], result, date, conv: null or [top, bottom]}]}]` newest first |
| `history_load` | `id` | frame (tap a row: loads that calculation) |
| `history_delete` | `ids: [..]` | true |
| `history_clear` | | true |
| `history_text` | `id` | string (long-press → Copy) |
| `copy` | | string (the shown number) |
| `paste` | `text` | `{ok, frame}` |
| `notes` | `utc_offset` | `[{id,title,preview,modified,date}]` newest first |
| `note_new` | | id |
| `note_get` | `id` | `{text, results:[LineResult], graphs:[[line,label]]}` |
| `note_set` | `id`, `text` | same as note_get (results for the new text) |
| `note_eval` | `text` | same, without saving |
| `note_delete` | `id` | true |
| `note_graph` | `text`, `line`, `x0`, `x1`, `n` | `[y or null]` |
| `icons` | | `{name: {prims:[Prim], stroke}}` |

### frame
```
{ theme: Theme, settings: Settings, display: Display, layout: Layout, convert: ConvState|null }
```
- **Display**: `main:[Seg]` the big line (while typing: the whole expression; after "=": the
  result), `expr:[Seg]` the small grey line above it (the expression after "="), `is_result`,
  `error`, `clear_label` "AC"/"C", `first_key`, `rad` (show a small "Rad" at the display's
  bottom-left when true), `second`, `memory`, `pending_op`, `value`, `empty`.
  `Seg{t, sup}`: draw `sup` segments raised and ~60% size.
- **Layout** (all in logical px, origin top-left of the window):
  - `keys:[KeyBox]` — `id`, `label:[Lab{t,v}]` (v: 1 raised small, -1 lowered small, 0 normal),
    `icon` (icon name instead of a label), `kind` (digit/func/op/sci/eq/mem → the theme's
    KeyStyle of the same name), `rect{x,y,w,h}`, `radius` (corner radius), `font` (label size),
    `active` (draw with `theme.active`).
  - `toolbar:[{id:"history"|"mode", icon, rect}]`
  - `display` rect: spans exactly the key grid's x-range. The numbers are right-aligned at
    `display.x+display.w-text_inset`. Modern themes: the big number's BASELINE sits exactly on
    the rect's bottom edge (the engine already left the iOS gap of ~0.22× the number size,
    15–26 pt, between that baseline and the keys) — offset by the font's own descent so the
    baseline, not the text box, lands there. Classic themes (`theme.display.boxed`): the number
    sits inside the LCD box near its bottom with `text_inset` padding. `main_font` is the big
    size (shrink further to fit the width, never wrap), `expr_font` the small line right above.
    `body_inner`/`title_h`: classic panel margin and the 1984 title bar height.
  - `conv_top`, `conv_bottom`, `conv_swap` (Convert mode): the input value row, the converted
    row (grey number), and where the ⇅ (icon "swap", accent colour) goes; a hairline divider
    (theme.separator) runs between the rows from the swap button to the right edge. Each row
    shows the number then the unit button: unit symbol + "updown" icon, small, grey.
  - `sidebar` rect (tablet history sidebar), `body` rect (classic themes: the device panel —
    fill theme.body→body2 vertical gradient, border theme.body_border), `scientific`,
    `landscape`, `tablet`, `menu_anchor` (where the mode menu pops from).
- **Theme**: colours are `0xRRGGBBAA` u32. Fills are vertical gradients `fill`→`fill2`;
  `border` (alpha 0 = none) is a 1 px outline; `gloss`: a white highlight over the top ~45% of
  the key (alpha ~0.35→0.05), `shadow`: a 2 px hard offset shadow (1984) in that colour.
  `pressed` is the fill while held. `display.boxed`: draw the display as a framed LCD box
  (bg→bg2 gradient, border, radius = radius×height). `glass_toolbar`: toolbar icons sit in 44 px
  dark circles (0x2C2C2E with a faint white border), else plain icons in `accent`.
  `font`: "system" (thin digits: use the platform's light weight; desktop may load a system
  font such as Inter/SF/Roboto Light from the OS at run time if present, never bundle Apple
  fonts), "chicago" → a bold monospace. `result_weight`/`label_weight` CSS-style weights.
  `light`: panels are light-themed.
- **Prim** (icons, 24×24 box, y down, stroke width `stroke`, round caps/joins):
  `{k:"Line",a,b}`, `{k:"Poly",pts,closed,fill}`, `{k:"Circle",c,r,fill}`,
  `{k:"Arc",c,r,a0,a1}` (radians, 0=+x, clockwise on screen), `{k:"RRect",min,max,r,fill}`.

### Key ids that are the app's job, not `press`
- `calc` (iOS 18 bottom-left key) and toolbar `mode`: open the **mode menu** at `menu_anchor`.
- toolbar `history`: open history (phone: panel; tablet: toggle `sidebar`).
- Long-press on the `ac` key (C) = `press("allclear")`.

## Behaviour and animation (both apps must match)
- **Key press**: on touch-down the key shows `pressed` at once. `theme.press`:
  `Glass` (iOS 26) — the key also scales to ~1.06 with a soft white glow, springs back on
  release (~350 ms spring); `Fade` — on release fade from pressed back to fill over ~250 ms
  ease-out; `Invert` (1984) — fill and text swap while held, no fade. Haptic tick on Android
  if `settings.haptics`.
- **Mode/size change** (basic↔scientific, rotation, sidebar, convert): keys animate from their
  old rect to the new one by `id` (~300 ms ease-in-out / spring); keys that appear fade+scale
  in from 0.8, keys that leave fade out. Font sizes animate too.
- **Display**: the big number shrinks to fit the width. On "=" the expression moves up into the
  small line while the result fades/rises in (~200 ms). Swipe left/right on the display =
  `swipe` (each swipe deletes one digit). Long-press the number → Copy/Paste menu.
- **Mode menu** (iOS 26 from the top-right button, iOS 18 from the bottom-left `calc` key):
  dark rounded popover (theme.panel, radius ~14), scales+fades in from its anchor (~200 ms):
  rows "Basic" (icon basic), "Scientific" (icon function), "Math Notes" (icon mathnotes) with a
  ✓ on the current one (iOS 26) or the current one in accent colour (iOS 18); separator; then
  "Convert" — iOS 26: a row with icon convert and ✓ when on; iOS 18: a switch. Then our extra
  row "Themes…" (icon paintbrush) → the theme picker / settings sheet. Tapping outside closes.
- **Convert mode**: two rows (see layout). Tapping a unit button opens the **Units sheet**:
  slides up from the bottom (~350 ms, iOS sheet curve), calculator behind dims (and on iOS 26
  scales to ~0.94 with rounded corners). Contents: title "Units" with a round accent ✓ Done
  button (iOS 26) or a "Done" text button (iOS 18), a horizontally scrolling row of category
  capsules (selected = filled accent capsule, white bold text; others accent text), the list
  of the category's units (name, symbol below in grey, ✓ + accent text on the selected one),
  separators, and a search field ("Search all units", magnifier) — iOS 26 at the bottom,
  iOS 18 at the top. Currency shows "Updated N minutes/hours ago" + source at the list end.
  Swipe down / Done closes. ⇅ swaps (row values animate/crossfade).
- **History**: phone — a panel slides in from the left covering ~85% of the width while the
  calculator slides right and dims (like docs/reference/7.png); header: list icon (closes) and
  "Edit". Groups ("Today", …) with rows: expression (grey, small) above the result (white),
  date at the right; conversions show the two conv lines. Tap a row → `history_load` and close.
  Swipe a row left → red Delete. Edit mode: selection circles (circle/checkcircle icons), "Delete"
  and "Delete All" at the bottom; Done ends. Long-press a row → Copy. Tablet — the same list
  in the left sidebar (`layout.sidebar`), toggled by the toolbar button.
- **Math Notes** (`mode: "Notes"`): a notes list (newest first, title + date + preview, "New
  Note" compose button) → a note editor. Every line that `note_set` reports as `result` shows
  its `text` in the accent colour right after the line's "=" (drawn as an overlay aligned to
  the text — not inserted into the text); `assign` lines show nothing extra; `error` shows
  "Error" in red after the "="; `graph` lines get a small graph chip; the note shows a graph
  view (axes, grid, accent curve per graph line; pan by drag, zoom by pinch/scroll) for its
  graph lines. Back returns to the list; the mode menu leaves Notes.
- **Settings / Themes sheet**: theme cards (mini preview from theme colours, name, year),
  number format (styles), haptics, sounds, and an about line.
