# Bramble Studio (working name)

A music app modelled on Logic Pro that opens Logic's own projects (`.logicx` packages and
project folders). First draft:

- **Reads Logic projects** (`lg-core`): tracks and their names, regions, MIDI notes, controllers,
  tempo, time and key signatures, Drummer regions, audio regions and their files, plug-in
  settings (Logic's own parameter blocks, the same as `.pst` presets).
- **Main window as Logic's**: control bar with LCD, Browser, Inspector, tracks area (headers,
  ruler, cycle, regions with notes / waveforms), Piano Roll, Mixer; Logic's menus.
- **Plays** with a SoundFont (General MIDI, played natively) or a built-in synth; audio regions.
- **Exports**: Bounce (WAV), MIDI file.

```sh
cargo run --release -p lg-app -- path/to/Song.logicx
cargo run --release -p lg-app --example bounce -- path/to/Song.logicx out.wav
cargo run -p lg-core --example dump -- path/to/Song.logicx
```

Keys (Logic's): Space play / stop, Return to the start, C cycle, X Mixer, E / P Piano Roll,
B Browser, I Inspector, M / S mute / solo the selected track, , and . a bar back / forward,
⌘← ⌘→ zoom, ⌘O open.

`LG_SNAP=out.png` draws the window and saves it (checks); research samples go in
`testdata-private/` (not published).

## Bundled fonts

- **Bravura** (crates/lg-app/fonts/Bravura.otf), the Score editor's music symbols (SMuFL), © Steinberg Media Technologies GmbH, under the SIL Open Font License 1.1 (crates/lg-app/fonts/Bravura-OFL.txt). Version 1.482 from github.com/steinbergmedia/bravura.
