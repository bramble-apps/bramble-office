# Bramble Studio sound library: sources and licences

The sampled instruments are built from these free libraries. Every licence below allows
redistribution inside a GPL-3.0 application, modification and format conversion. The audio was
converted to 16-bit FLAC, trimmed (fewer velocity layers, shorter tails), mixed down (drum kits:
close mics, overheads and room to stereo), looped (sustained orchestral notes: a crossfade baked
into the sample) and re-mapped to new SFZ files. The SFZ files in `sfz/` and the tools in `tools/`
are part of Bramble Studio (GPL-3.0-only); the samples keep their own licences, listed here.

Licences were checked against the licence text each project publishes (date checked: 2026-11-10).

| Instruments | Library | Version | Licence |
|---|---|---|---|
| drums-modern, drums-brush | DRSKit (DrumGizmo) | 2.1 (Aug 2018) | CC-BY 4.0 |
| drums-vintage, shared hand percussion | Virtuosity Drums (Versilian Studios, Karoryfer Samples) | 0.925 | CC0 1.0 |
| piano-grand | Salamander Grand Piano | V3 (2011-08-12), 44.1 kHz/16-bit set | CC-BY 3.0 |
| strings, brass, woodwinds, harp, timpani | VS Chamber Orchestra 2: Community Edition (Versilian Studios) | 1.1.0 (master/SFZ branches) | CC0 1.0 |
| vibraphone, marimba, xylophone, glockenspiel, tubular bells, kalimba, tenor sax, claps, cajon | Versilian Community Sample Library (VCSL) | master/sfz branches (v1.2.2-RC era) | CC0 1.0 |
| guitar-electric | Emilyguitar (Karoryfer Samples) | 1.001 | CC0 1.0 |
| bass-electric | Fashionbass (Karoryfer Samples) | 1.001 | CC0 1.0 |
| bass-upright | Meatbass, pizzicato (Karoryfer Samples) | 1.001 | CC0 1.0 |
| guitar-nylon | Spanish Classical Guitar (FreePats) | 2019-06-18 | CC0 1.0 |
| guitar-steel | FSS Steel-String Acoustic Guitar (FreePats) | 2020-05-21 | GPL-3.0-or-later with the FreePats exception |

## DRSKit 2.1 — drums-modern, drums-brush

- Source: https://drumgizmo.org/wiki/doku.php?id=kits:drskit
  (archive https://drumgizmo.org/kits/DRSKit/DRSKit2_1.zip, md5 8c4d4b61ad9d354b3b845edd5da9c133)
- Licence: Creative Commons Attribution 4.0 International,
  https://creativecommons.org/licenses/by/4.0/ — the kit's README: "This kit is distributed under
  the Creative Commons Attribution 4.0 International license".
- Attribution: **DRSKit by the DrumGizmo team (Lars Muldjord, Bent Bisballe Nyeng, Deva) and
  Jes Eiler of DRSDrums (www.drsdrums.dk), www.drumgizmo.org. Licensed under CC-BY 4.0.**
  Changes: each hit mixed from its 13 microphones to stereo, a subset of the velocity samples,
  tails trimmed, 16-bit FLAC; the "whisker" (brush) articulations make the brush kit.
- The DrumGizmo site also asks that music made with the samples credit "Drum samples provided by
  DrumGizmo.org" where credits are given.

## Virtuosity Drums 0.925 — drums-vintage, hand percussion in every kit

- Source: https://versilian-studios.com/virtuosity-drums/ (download
  https://github.com/sfzinstruments/virtuosity_drums/releases/download/v0.925/Virtuosity_Drums_v0.925.zip)
- Licence: CC0 1.0 Universal (LICENSE file in the release; the project page: "an open source
  (Creative Commons 0) contemporary Jazz drum kit library").
- Credit (not required): Versilian Studios and Karoryfer Samples; auxiliary percussion from VSCO 2
  Pro and Karoryfer.

## Salamander Grand Piano V3 — piano-grand

- Source: https://archive.org/details/SalamanderGrandPianoV3
  (SalamanderGrandPianoV3_44.1khz16bit.tar.bz2)
- Licence: Creative Commons Attribution 3.0 Unported, http://creativecommons.org/licenses/by/3.0/
  (archive.org licence field and the description: "Licence: CC-by").
- Attribution: **Salamander Grand Piano V3 by Alexander Holm (Yamaha C5), CC-BY 3.0.**
  Changes: 6 of the 16 velocity layers, tails trimmed to 14 s, string-resonance release layers
  left out, 16-bit FLAC.

## VS Chamber Orchestra 2: Community Edition — strings, brass, woodwinds, harp, timpani

- Source: https://github.com/sgossner/VSCO-2-CE (samples: `master`, mappings: `SFZ` branch),
  https://versilian-studios.com/vsco-community/
- Licence: CC0 1.0 Universal (LICENSE file in the repository).
- Credit (not required): recorded by Sam Gossner and Simon Dalzell, sample cutting by Elan
  Hickler / Soundemote; Versilian Studios.

## Versilian Community Sample Library (VCSL) — mallets, tenor sax, claps, cajon

- Source: https://github.com/sgossner/VCSL (samples: `master`, mappings: `sfz` branch),
  https://versilian-studios.com/vcsl/
- Licence: CC0 1.0 Universal ("This collection is under a Creative Commons 0 license").
- Credit (not required): Versilian Studios and contributors.

## Karoryfer Samples — Emilyguitar, Fashionbass, Meatbass

- Sources (linked from https://shop.karoryfer.com free pages):
  - https://github.com/sfzinstruments/karoryfer.emilyguitar (v1.001)
  - https://github.com/sfzinstruments/karoryfer.fashionbass (v1.001)
  - https://github.com/sfzinstruments/karoryfer.meatbass (v1.001)
- Licence: CC0 1.0 Universal (LICENSE file in each release; readmes: "Royalty-free for all
  commercial and non-commercial use", Meatbass: "open source, and you can do whatever you want
  with it").
- Credit (not required): played and mapped by D. Smolken; Meatbass recorded by Ludwik Zamenhof.

## FreePats Spanish Classical Guitar — guitar-nylon

- Source: https://freepats.zenvoid.org/Guitar/acoustic-guitar.html
  (SpanishClassicalGuitar-SFZ+FLAC-20190618.7z)
- Licence: CC0 1.0 ("Published under the terms of Creative Commons CC0 public domain dedication").
- Credit (not required): roberto@zenvoid.org, FreePats project.

## FreePats FSS Steel-String Acoustic Guitar — guitar-steel

- Source: https://freepats.zenvoid.org/Guitar/steel-acoustic-guitar.html
  (FSS-SteelStringGuitar-SFZ-20200521.tar.xz)
- Licence: GNU GPL version 3 or later, with this special exception (from the package readme):

  > As a special exception, if you create a composition which uses these sounds, and mix these
  > sounds or unaltered portions of these sounds into the composition, these sounds do not by
  > themselves cause the entire composition as a whole to be covered by the GNU General Public
  > License. This exception does not however invalidate any other reasons why the composition
  > might be covered by the GNU General Public License.
  >
  > If you modify these sounds, you may extend this exception to your version of the sounds,
  > but you are not obligated to do so. If you do not wish to do so, delete this exception
  > statement from your version.

  We extend the exception to our modified version.
- Copyright: 2008 Gary Campion (FlameStudios, "FS Seagull Steel String Acoustic Guitar"),
  modified 2016–2020 by roberto@zenvoid.org for FreePats (the author gave FreePats permission to
  distribute under this licence; the readme quotes his e-mail). Modified again for Bramble Studio
  (16-bit FLAC, re-mapped).

## Considered and left out

- University of Iowa MIS, Philharmonia Orchestra samples: no formal licence text (Iowa), or
  terms that restrict redistributing the samples as such (Philharmonia).
- Sonatina Symphonic Orchestra: Creative Commons Sampling Plus 1.0 (retired, restricts
  redistribution of the samples themselves).
- AVL Drumkits: a "slightly modified" CC-BY-SA whose exact text we could not read.
- Salamander Drumkit: CC-BY-SA 3.0 — usable, but DRSKit (CC-BY) covers the same need.
- jRhodes3 (electric piano): samples are CC-BY-NC — non-commercial, excluded.
- Greg Sullivan's E-Pianos (Wurlitzer, CP80, Pianet; CC-BY 4.0 on the author's site): 1999–2004
  GigaStudio files on Google Drive, small and looped; the modelled electric pianos stay.
- D. Smolken double bass: listed as CC0 in one place and GPL-3 in another; Meatbass (CC0) is used.
- No choir library with a clear free licence was found; the choir stays synthesized.
