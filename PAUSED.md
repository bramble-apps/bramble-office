# studio-focus — paused 2026-11-12

## Bug reproduced (headless, before the fix; demo song, Piano Roll on "Deluxe Classic")
- Click a note, Backspace with the pointer over the Piano Roll: nothing was deleted (the Piano Roll never got Delete).
- Click a note, move the pointer over the tracks, Backspace: the whole region was deleted (it fills the track, so it looked like the track was gone).
- ⌘A with the pointer over the tracks selected every region, so Delete then emptied them all.
- Ctrl+Backspace (Logic's ⌘⌫) is "Delete Track", as in Logic. That is still the case.
- keymap: ⌃ bindings fired on the bare key (plain ⌫ ran "Delete Redundant Automation Points").

## Finished
- focus.rs: Logic's key focus. One field (`App::kf`) is set by the last click in an area (Tracks, Editor, Mixer, Browser, Event List, Step Editor, or Other for windows, Smart Controls and Project Audio). Menus and popovers keep the focus. ⇥ / ⇧⇥ (Cycle Through Window Views) cycle it, and egui's Tab widget focus is undone afterwards. Logic's 2 pt blue frame (106,140,184 @ 0.76, measured on ui-tracks-with-pianoroll.png) is drawn on the focused area.
- Every pointer-based key check is replaced: notes_focus, the tool choice, the tracks.rs hover Delete (removed), midifunc roll_ctx (roll_focus removed), stepseq Copy/Paste, audioedit in_editor, evlist::focused, the browser, projaudio and smartcontrols key_press, the score symbol Delete, the arranging tempo Delete, markers Delete (moved into markers::command), ddview, and the stmulti, scoregrid and scorestyle windows (focus::in_window).
- Delete / Backspace go through App::keys → focus::delete_key → Edit ▸ Delete for the focused area. With an editor focused, Delete removes the selected notes (or steps in the Step Sequencer) and never regions.
- ⌘C / ⌘X / ⌘V: egui-winit sends Event::Copy/Cut/Paste and no key. keys() now maps them back to the key commands, so before this nothing happened in the real app.
- Select All in the Tracks area now selects every region (app.rs's own arm only selected the first region of a track).
- Copy, Cut and Paste of regions (regionclip.rs) cover every selected region on every track, keep relative tracks and distances, keep the audio extras, and are one undo step.
- Region resizing (regiontrim.rs): there is a left-edge trim for every region type (start, length and audio file offset move together, so the audio stays at the same time and can't go before the file start). The right edge stops at the file end unless Option is held. Hidden MIDI notes are no longer drawn piled at the region start. The Tracks-area waveform is drawn from the file at audio_offset, at the timeline's scale (flex, gain and fades as in the audio editor), with the engine render (also at true scale) as fallback.
- snap.rs: LG_IMPORT=/file.wav imports an audio file (for headless checks).
- Unit tests: focus.rs (3), regiontrim.rs (3), regionclip.rs (2), keymap (⌃ fix).

## Half-done / not verified
- Waveform and trim are built but not checked headless yet. Next: LG_IMPORT=/tmp/claude-1000/stf/beats.wav, select the region, drag its left and right edges, and snapshot.
- Left-edge Option-drag doesn't time-stretch yet (right edge does, through stretch.rs). Pattern and Session Player region contents aren't checked after a left trim.
- Playback window: audioedit::playback already renders offset..offset+region length (same window as the drawing). Confirm by ear or with a render test.
- Real-keyboard ⌘V with an empty system clipboard sends no event at all (egui-winit), so Paste does nothing in that case.
- Critic pass on the focus frame: colour and width are measured. The light theme isn't checked.
- Audit not done yet: Option-drag copy, double-click to open, arrow keys in Score / Event List / Step Sequencer, Undo/Redo of each.
- CHECKLIST.md audit lines and tool/checklist.py not run yet.

## Headless setup
/tmp/claude-1000/stf/snap.sh (weston socket stfw, XDG_CONFIG_HOME isolated). Scenarios are in this session's notes: click note (1070,760) and region (1250,340) at 1400x900 with LG_SELECT="2 0" LG_EDITOR=roll.
