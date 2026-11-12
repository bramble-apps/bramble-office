# studio-layout — paused 2026-11-12

## Finished (committed)
- barfit.rs: `fit()` layout rule (controls leave in a fixed order, "…" only when something left), tested.
- Tracks local bar: Logic 13's full set, all working (↑ Go Out of Folder, Edit/Functions/View,
  grid|tracks, Flex, Automation, Catch, Left/⌘ tool menus, Snap on/off + Snap pop-up, Drag pop-up,
  Waveform Zoom, Vertical/Horizontal Auto Zoom, V/H zoom sliders); overflow order matches Logic's
  630 pt bar. Catch hooked into nav::follow_playhead (`app.fit.catch`).
- Piano Roll bar and Mixer bar laid out by width (40 pt); Score bar 40 pt.
- Control bar: groups drop individually; 38 pt buttons; Logic-drawn icons (cbicons.rs: Browser,
  Inspector, Smart Controls, Low Latency, Capture, Browsers, Flex); square Stop; ph::METRONOME.
- Browser header/search measured; sidebar collapses to icons when labels don't fit; footer volume
  no longer overlaps; Inspector disclosure names ellipsized.
- edpane.rs: editor pane opens the selected track's region when none is selected; Session Player
  regions get Session Player | Piano Roll | Score tabs; tabs 127 pt × 24 pt. Tested.
- Crops in ~/CascadeProjects/st-layout-shots/.

## Half-done / not done
- Piano Roll / Mixer full-width button sets are Studio's guess (Logic's not in research); no Link.
- Control bar has no "…"; Logic 13 default transport shows Capture + one more button (not changed).
- Empty Piano Roll (track without regions) still shows a message; Drummer keeps "Session Player" title.
- Not re-laid-out: Notes / Loops / List Editors panes, Inspector width, Score bar overflow.
- Control-bar icon critic done once (crops ref-controlbar-*.png vs Studio); a second pass
  after the 38 pt spacing change was not run.

## Next steps
1. Critic pass on control bar icons after the spacing change; Score bar overflow via barfit.
2. Logic's empty Piano Roll; 4-tab set (Piano Roll | Score | Step Sequencer | Session Player)
   for an instrument track with no region.
3. Right panes (Notes, Loops, List Editors) headers by width.
4. `python3 tool/checklist.py`, one `cargo build --release -j 1` at the end.
