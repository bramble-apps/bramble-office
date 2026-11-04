# Bramble Notes editor: parity with Apple Notes 4.13

What the editor (crates/nt-app/src/editor*) does next to Apple's, area by area. Sources: the NO1
feature map and interactions (notes-research-src/partial-3/Notes-Research/features, interactions),
the menu tree, the context menus, and the NO2/NO3 rounds. **done** = built and checked against
Apple's captures, **tested** = unit/monkey tested here, **skipped** = left out, with the reason.

Always on: `every_editor_menu_item_is_handled` walks Notes' whole menu bar and sends each item the
app routes to the editor; `monkey_on_the_library` (on a library copy) runs random typing, keys,
clicks, drags, the wheel and menu commands; `big_notes_open_fast` measures frames.

## Speed and stalls
| | status |
|---|---|
| Layout: only paragraphs in view, cached between frames, edited ones again | done, tested (pixel-identical to a full layout on all research notes) |
| Pictures, PDF pages, older PDFs' page sizes on low-priority workers | done, tested |
| Drawings' fallback pictures drawn on a worker | done, tested |
| Opening a note changes nothing in it | tested (all 1225 notes of the user's library copy) |
| Worst frame on the user's library | 8.6 ms (was 867 ms); monkey, six seeds × 200 notes × 300 steps: no panic, worst 24 ms (a 3,000-stroke drawing), else 17 ms or less |
| Spell check off the UI thread | done: paragraphs checked on the workers, cached by text |
| One huge paragraph (50,000 characters) laid out in one go when reached | limit of egui's text layout (one galley per paragraph) |

## Paragraphs, lists, auto-format
| | status |
|---|---|
| Title / Heading / Subheading / Body / Monostyled, ⇧⌘T H J B M | done |
| Bulleted / Dashed / Numbered lists, ⇧⌘7 8 9; indents ⌘] ⌘[; Return continues, empty item ends | done |
| Checklist ⇧⌘L, Mark as Checked ⇧⌘U, Check All / Uncheck All / Move Checked to Bottom / Delete Checked, auto-sort, tick animation | done (NO2 timings) |
| Block Quote ⌘', Move Item ⌃⌘↑/↓, Alignment ⌘{ ⌘| ⌘}, Justify | done |
| Writing Direction (paragraph; selection = its paragraphs, egui has no bidi runs) | done, tested |
| Collapsible headings, Expand/Collapse Section and All | done |
| Auto-format: "- " "* " "• " "1. " "1) ", em dash, … , smart quotes, Smart Links, Smart Tags, Math | done (NO1 27 Autoformat) |
| Divider ⌘L | done |

## Characters
| | status |
|---|---|
| Bold, Italic, Underline, Strikethrough, Highlight (5 colours), Bigger/Smaller, Baseline | done |
| Copy Style / Paste Style / Remove Style | done |
| Show Fonts (font, typeface, size) / Show Colors (text colour) | done (panels in place of the Mac's), tested |
| Transformations: Upper / Lower / Capitalize | done |

## Tables
| | status |
|---|---|
| Add Table ⌥⌘T, Tab / Shift-Tab between cells, Tab in the last cell adds a row | done, tested |
| Row and column handles, Add Row Above/Below, Add Column Before/After, Delete | done |
| Convert to Text; copy and paste as a table | done, tested |
| Column widths: dragged here kept | done |
| Column auto-sizing as Apple's (content-fitted, min/max, wrapping, sideways scroll) | waiting on a Mac round (Apple stores no widths: notes-format) |
| Drag row / column handles to reorder (live, one undo step) | done, tested |
| Merge cells | skipped: Notes has none (no such string) |

## Links, tags, Math, data detectors
| | status |
|---|---|
| Add Link ⌘K (URL or note, Link to Section, Use Note Title), Edit/Open/Copy/Remove Link | done |
| ">>" note-link autocomplete; note links open the note at the section | done |
| Tags "#word", autocomplete, Tags menu | done |
| Math: results, Suggest / Insert / Off, variables, brackets, implied multiplication | done |
| Math units: sums of quantities, conversions (in / to / as), length, mass, volume, time, area, speed, data, temperature | done, tested; result rounding (4 decimals) not measured against Apple |
| Math currency | skipped: needs live exchange rates (online) |
| Data detectors (dates, phone numbers, addresses): found, dotted, right-click Call / Send Message / Show in Maps / Copy | done, tested; the look is not measured against Apple's yet (asked for a Mac capture) |

## Find, spelling
| | status |
|---|---|
| Find ⌘F, Find and Replace ⇧⌘F, Next/Previous, Use Selection, Jump to Selection | done |
| Check Spelling While Typing, suggestions, Learn / Ignore | done |
| Check Document Now ⌘; / Show Spelling and Grammar ⌘: | done, tested |
| Grammar | as kn-ui's spell module (basic rules) |

## Edit, export
| | status |
|---|---|
| Cut / Copy / Paste, Paste and Match Style, Paste and Retain Style | done |
| Copy as Markdown / Paste as Markdown | done, tested |
| Print, Export as PDF / Markdown | the app's (shell) |
| Text size (Settings), Zoom In/Out/Actual Size | done |
| Open Note in New Window | the app's (shell) |

## Attachments
| | status |
|---|---|
| Pictures: View As Small (card) / Large, Rename Attachment…, Quick Look / Open, Set All to Small/Large | done, tested |
| PDFs (Notes 4.13 viewer): Small/Medium/Large, thumbnails, Quick Look, Rename | done (NO3 PDF) |
| Files: card, Rename | done |
| URL cards | done |
| Scans: first page shown | done; paging through a scan's pages: waiting on a Mac capture of a multi-page scan |
| Audio: a card with Play / Pause, its length, the bar and time while playing, a click on the bar seeks; m4a, mp3, wav, aac, flac, ogg attach as audio | done, tested (sound itself not heard here); the card's look not measured against Apple's (no capture); transcripts not decoded by nt-core yet |
| Not-downloaded iCloud attachments: a card with their kind's icon | done, tested |
| Drawings (PaperKit): every ink, eraser, shapes, text, signatures, Apple's drawings read | done (NO3) |
| Markup on pictures | done |
| Markup on PDFs | waiting on nt-core reading and writing page annotations (asked notes-format); greyed meanwhile |
| Rotating pictures | skipped: Notes on the Mac has no rotate for an attachment (Markup only) |

## iCloud and Apple services (skipped: proprietary)
Sharing / collaboration, mentions, Show Highlights, Writing Tools, AutoFill, Insert from iPhone,
Dictation, Speech, Quick Note hot corner.
