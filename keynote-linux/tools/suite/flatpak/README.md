# Flathub packaging

Keydeck, Letters, Values, Bramble Calculator and Bramble Notes as Flatpaks for Flathub (and so for
GNOME Software, KDE Discover and Bazaar). App ids are under `io.github.bramble_apps`, which Flathub
verifies against the `bramble-apps` GitHub organisation.

| App | App id | Workspace | Package | Binary |
|---|---|---|---|---|
| Keydeck | io.github.bramble_apps.Keydeck | keynote-linux | kn-app | keydeck |
| Letters | io.github.bramble_apps.Letters | pages-linux | pg-app | letters |
| Values | io.github.bramble_apps.Values | numbers-linux | nm-app | values |
| Bramble Calculator | io.github.bramble_apps.Calculator | calculator | calc-app | calculator |
| Bramble Notes | io.github.bramble_apps.Notes | notes-linux | nt-app | bramble-notes |

## Files

- `make_flatpak.py meta .` writes the `.desktop` and metainfo files here. `export.sh` copies them
  into bramble-office's `flatpak/`, where the manifests install them from.
- `make_flatpak.py submit <dir> <bramble-office commit>` writes one folder per app, as its Flathub
  repo holds it: the manifest (building bramble-office at that commit), `<workspace>-cargo-sources.json`
  (every crate, for the offline build, from `cargo_sources.py`) and the modules it uses.
- `ffmpeg.json`, `poppler.json`: the programs the apps call that the runtime lacks (ffmpeg for audio
  and video; poppler's `pdftoppm` and `pdfinfo` for PDF pages). ffmpeg 9.0.2 and poppler 26.10.0;
  their `x-checker-data` lets Flathub's bot propose updates.

Each app takes the app id as its Wayland app id when `FLATPAK_ID` is set, so the desktop matches the
window to its icon.

## Not in the Flatpaks

These call programs a Flatpak can't reach, so the features behind them behave as on a system
without them (not yet checked inside the Flatpak): `pdflatex` (equations as LaTeX), `spd-say`
(speaking text), `arecord`/`pw-record` (recording audio), `lp`/`lpstat` (printing) and
`fprintd-verify` (Notes' fingerprint unlock). Moving recording to PipeWire through the portal and
printing to the print portal would bring them back.

## Releasing

1. Commit here, run `export.sh ~/CascadeProjects/bramble-office "<message>"` and push bramble-office.
2. `make_flatpak.py submit ~/CascadeProjects/flathub-submit <the pushed commit>`.
3. Build and test each: `flatpak run org.flatpak.Builder --user --install --force-clean build <id>.yml`,
   then `flatpak run org.flatpak.Builder --run build <id>.yml <binary>`; lint with
   `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest <id>.yml`.
4. First submission: fork github.com/flathub/flathub, branch from `new-pr`, add the app's folder's
   files, open a pull request against `new-pr`. Once accepted, Flathub makes github.com/flathub/<id>;
   later releases are pull requests there with a new commit and regenerated cargo sources.
5. Screenshots: the metainfo files point at `https://bramble-apps.github.io/screenshots/<app>-1.png`
   (put them in bramble-site before submitting).
