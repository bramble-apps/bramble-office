# Paused 2026-11-12 (studio-transport)

Finished and committed (b79dc9a; debug build OK, lg-app lib 328/328 and lg-core 181/181 tests pass):
- Project End: playback stops at it (Cycle loops first; Stop Button Options' locate applies after
  the stop; Live Loops cells keep the song running), recording past it extends it, read/written at
  Song +0x1A4 (Logic's Empty Project = bar 129), Bounce runs to it, ruler end marker (mirrored start
  triangle, start triangle re-measured), drag by bars with help tag + undo. Files: lg-core/src/projend.rs,
  lg-app/src/projend.rs, hooks in audio.rs, app.rs, tracks.rs, share.rs, liveloops.rs, save.rs, project.rs.
- Browser previews (lg-app/src/brpreview.rs, hooks in browser.rs, preview.rs): icon click start/stop,
  Option-click / Right Arrow retrigger, selected-sound toggle, drag / Option-hover sweep, speaker icon,
  looped loops, Play Preview on Selection per view, patch chord C3-E3-G3 1.5 s rendered offline.
- apple-guide/status-transport.json written and merged (16 entries).

Not done / next steps:
- The final `cargo build --release -j 1` was stopped by the pause: run it.
- Project Start can't move before bar 1; no LCD Project End display (Custom LCD).
- Preview Player waveform, Preview Volume pop-up button, column-layout arrow keys.
- Not heard by ear (no audio device in headless runs): chord preview loudness/sound, auto-stop in the live app.
