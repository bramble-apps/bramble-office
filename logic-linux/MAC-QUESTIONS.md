# Questions for the Mac (open in real Logic Pro 13 to confirm)

Files Studio wrote that still need opening in Logic. For each: make the file on Linux with the
steps given, copy it to the Mac, open it in Logic, and note what Logic shows (or whether it
refuses the file). Studio's round-trip tests (`cargo test -p lg-core writeback bandsave package`)
only prove that Studio reads back what it wrote.

## Save (lg-core save.rs, writeback.rs)

1. **Track name** (E23 copy: rename "Deluxe Classic" → "Keys Renamed", Save). Does Logic show the
   new name in the header, the Mixer and the Inspector? (Envi payload 158, §15.)
2. **Region name and Region inspector values** (E23 copy: region "RenamedRegionXY" renamed,
   Loop off, Velocity −5, Transpose −12). Does the Region inspector show them; does the name
   lose the "(+3)" suffix? (MSeq name; placement record byte 13 bit 0x02, bytes 52 / 53, §14.)
3. **Fader / pan / mute / solo** (E23 copy: one track's fader to fader value 70, pan +16, mute
   toggled, solo on). Studio writes AuCO @0x79 / @0x7D / @0x7E / @0x7C only. Does Logic take the
   fader from @0x79, or from the float at @0x98 (it changed to 1.5591 when Logic set −6 dB)?
   Does solo need the other strips' "implicitly muted" 0x02 bits Logic writes (§24)?
4. **Aux strip fader / mute** (E07 copy: Aux 1 to −10 dB, muted).
5. **Group membership and settings** (E24 copy: Audio 1 → Group 2, Audio 2 → no group; Group 1
   mask 07 01 40 81 with its 0x50 records for Volume, Pan, Mute). Do the Group Settings
   checkboxes match? Group 2 has no settings sequence in E24: is membership alone enough?
6. **Plug-in parameters** (E04 copy: Channel EQ band 4 gain +6 dB). Floats written into the
   GAMETSPP block only; is anything else (a checksum, the AuCO byte that changed on insert)
   needed?
7. **Time signature and key** (E02 copy: 3/4, F minor). Signature list first 0x30 record
   (bytes 11 / 12 and the bar offset at 24 = −13), Song +0x39E, key byte 0x13, MetaData.plist.
   Does the LCD show 3/4 Fm and the ruler 3/4 bars?
8. **Tempo changes** (E23 copy: a third tempo 90.5 BPM at bar 13). Records modelled on the
   existing ones, the cached time at +24 worked out as half-milliseconds from 01:00:00:00
   (checked against E23's two). Does the Tempo track show the step at bar 13?
9. **Markers** (E00 copy: markers "Top" at bar 1 and "Bridge" at bar 3; E23 copy: arrangement
   "Verse" renamed "Verse 1", "Outro" added at bar 25, 4 bars). New names are plain-text TxSq
   chunks (kind 0x14), also for normal markers where Logic itself writes RTF. Does Logic show
   them? Removed markers leave their TxSq behind: does Logic mind?
10. **New MIDI region** (E02 copy: a region drawn at bar 9 with one note, transpose +2). Made
    from a copy of the track's own region sequence (MSeq/Trak/EvSq), its events replaced.
11. **New audio region on a file the project has** (R01 copy: composite-40s copied to bar 21,
    fade-in 250 ms) and **a recorded file** (R01 copy: a WAV recorded in Studio into
    Media/Audio Files). New AuRg (and AuFl) chunks copied from the project's own, with the name,
    file window, frames / rate / channels / bits / data offset written in; AuRg @0x2A stamp
    changed, the 16-byte UUID further on is NOT changed (duplicated from the model). Does Logic
    play both? Does it complain about the duplicate UUID?
12. **Audio region trims and fades** (R01 copy: region start moved 1 s into the file, 1 beat
    shorter, fade-in 250 ms curve −10, no fade-out). AuRg @0x06 / @0x16, record unit 5.

## Project management (lg-core package.rs, lg-app projfile.rs)

13. **New Alternative** (E24 copy: New Alternative… "E24 #1", then Edit Alternatives ▸ Rename
    "Bridge idea"). ProjectInformation.plist VariantNames / VariantNamesV2 / ActiveVariant and
    Alternatives/001. Does Logic open on "Bridge idea" and list both?
14. **Export Alternative as Project** (the package above, "Bridge idea" exported).
15. **Project File Backups** (any copy saved three times in Studio): does File ▸ Revert to list
    Studio's backups (slots 00..09, ProjectData + MetaData.plist)?
16. **Save .band** (a copy of an iOS GarageBand project: one note moved, tempo changed). The
    archive is rewritten as XML with the new model bytes; open it in GarageBand for iOS and in
    Logic (File ▸ Open).

## Not written (Save names these): decode samples needed

- New tracks (Envi + AuCO + AuCU + Trak for a new channel; E00 → a project with one more track)
- Deleted tracks, track order, track and region colours
- Plug-ins added / removed / replaced, bypass (one save per change)
- Send levels (E07 with the send at −12 dB: which bytes change in the 112-byte AuCU)
- Outputs and inputs, new auxes, new groups, group names, VCAs
- Region gain, fade type, quantize / delay / dynamics / gate time, curves and plug-in
  automation, sample rate in ProjectData
