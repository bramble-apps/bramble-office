## Desktop

Pure Rust (egui/eframe), in `crates/calc-app`; the binary is `calculator`.

```sh
cargo run -p calc-app --release        # or: cargo build -p calc-app --offline
```

Settings, history and notes are kept in `$XDG_DATA_HOME/app.bramble.calculator/`
(default `~/.local/share/app.bramble.calculator/`). No fonts are bundled: the app picks a system
sans at run time (SF Pro, Inter, Ubuntu Sans, Roboto, Cantarell, DejaVu, … variable fonts give the
thin digits); `CALC_FONT=/path/font.ttf` overrides it. Currency rates are fetched with `curl`
when Convert opens and the rates are older than six hours.

Keyboard: digits and `+ - * / x ^ % ( ) ! .`, Enter = `=`, Backspace = ⌫, Esc = AC (Shift+Esc =
all clear; Esc also closes menus and sheets), Ctrl+C / Ctrl+V copy and paste the number.
Mouse: swipe across the number to delete a digit, right-click or hold it for Copy/Paste, hold
C for all clear, right-click a history row to copy it.

### Test hooks (environment variables)

| variable | effect |
|---|---|
| `CALC_SIZE=WxH` | initial window size in logical px (e.g. `844x390` landscape, `1180x820` tablet) |
| `CALC_THEME=id` | `ios26`, `ios18`, `mac2019`, `iphone2007`, `aqua2001`, `mac1984` |
| `CALC_KEYS=1,2,add,3,eq` | key ids pressed one per frame after start. Extras: `@sci`, `@basic`, `@notes`, `@convert`, `@noconvert`, `@unit:cat:from:to`, `@style:Dot`, `@sidebar:off`, `@caffeine` (new Math Note with the caffeine example, opened), `@clearhistory`, `@wait:secs` (pause before the next key), `@resize:WxH` (resize the window) |
| `CALC_OPEN=…` | `history`, `history-edit`, `menu`, `units`, `settings`, `notes`, `note` (newest note) |
| `CALC_SNAP=/path.png` | after the keys and ~1 s of settling, save a screenshot and quit |
| `CALC_DATA=/dir` | use a scratch data folder |
| `CALC_PROF=1` | print frame CPU times once a second (update, whole frame) and any slow frame |

```sh
timeout 20 env CALC_DATA=/tmp/calc CALC_SIZE=390x760 CALC_KEYS="1,0,2,4,mul,4,eq" \
  CALC_SNAP=/tmp/calc.png target/debug/calculator
```

Snapshots: `docs/snapshots/desktop-*.png`.
