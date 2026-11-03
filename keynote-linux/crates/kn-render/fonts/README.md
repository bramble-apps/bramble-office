# Bundled fonts

Free metric-compatible stand-ins that Word documents need to lay out exactly as OnlyOffice
(and Word) lay them out, and that draw the fonts people expect in a font menu (Arial, Times
New Roman, Courier New, Calibri, Cambria) where those aren't installed (Android). Built into every suite app by kn-render (`Fonts::new`).

| Font    | Stands in for     | Licence                    |
|---------|-------------------|----------------------------|
| Carlito | Microsoft Calibri | SIL Open Font License 1.1 (`carlito/OFL.txt`) |
| Caladea | Microsoft Cambria | SIL Open Font License 1.1 (`caladea/OFL.txt`) |
| Liberation Sans / Serif / Mono | Arial / Times New Roman / Courier New | SIL Open Font License 1.1 (`liberation/OFL.txt`) |

`matched/` holds metric-matched stand-ins for Apple fonts (Sfera for SF Pro, Helvetia Neue,
Avelon, Futurae …): a Pages / Keynote / Numbers document naming an Apple font that isn't
installed is laid out with them, line for line as on a Mac. Its README lists them and their
licences; they are read in place from the program (`src/fonts_matched.rs`, generated).

The files are the upstream releases (Carlito and Caladea as OnlyOffice Desktop Editors ships
them; Liberation 2.1 as Debian's fonts-liberation ships it). The OFL allows
bundling and redistributing them with software; they are not to be sold on their own, and the
licence travels with them.
