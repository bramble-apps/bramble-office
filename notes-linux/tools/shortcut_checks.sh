#!/bin/bash
# Scripted checks of the note list's multi-selection, its keyboard shortcuts and the library's
# undo (on the demo library, nothing written to disk). Usage: tools/shortcut_checks.sh [binary]
B=${1:-$(dirname "$0")/../target/debug/bramble-notes}
pass=0; fail=0
check() { # name, script, expected text in the NT_DUMP selection line
    out=$(NT_DUMP=1 NT_SNAP=/dev/null NT_SIZE=1065x836 NT_NOTITLE=1 NT_SCRIPT="wait; $2; wait; wait" timeout -s KILL 90 "$B" 2>&1 | grep -E "^(selection|view):" | tr '\n' ' ')
    if [[ "$out" == *"$3"* ]]; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1: wanted '$3' in: $out"; fi
}
# rows of the demo's Bramble Research list: 33 Drawing at y 290, 30 File at y 402
check "shift-click selects a range"      "click 320 290; shiftclick 320 402"                    "selection: 3 "
check "ctrl-click adds a note"            "click 320 290; ctrlclick 320 402"                     "selection: 2 "
check "ctrl-click takes it out again"     "click 320 290; ctrlclick 320 402; ctrlclick 320 290"  "selection: 1 "
check "shift+down extends"                "click 320 290; key shift+ArrowDown; key shift+ArrowDown" "selection: 3 "
check "shift+end to the last note"        "click 320 290; key shift+End"                         "selection: 34 "
check "ctrl+A in the list selects all"    "click 320 290; key ctrl+A"                            "selection: 36 "
check "delete deletes the selection"      "click 320 290; shiftclick 320 402; key Delete"        "deleted: 3 "
check "undo Delete Notes"                 "click 320 290; shiftclick 320 402; key Delete; wait; key ctrl+Z" "deleted: 0 "
check "the undo is labelled"              "click 320 290; key Delete"                            'undo: Some("Delete Note")'
check "redo deletes again"                "click 320 290; shiftclick 320 402; key Delete; wait; key ctrl+Z; wait; key ctrl+shift+Z" "deleted: 3 "
check "pin the selection"                 "click 320 290; shiftclick 320 402; menu Pin Note"     "pinned: 4"
check "undo Pin Notes"                    "click 320 290; shiftclick 320 402; menu Pin Note; wait; key ctrl+Z" "pinned: 1"
check "drag the selection to a folder"    "click 320 290; shiftclick 320 402; drag 320 300 100 290" 'undo: Some("Move Notes")'
check "undo Move Notes"                   "click 320 290; shiftclick 320 402; drag 320 300 100 290; wait; click 320 600; key ctrl+Z" 'redo: Some("Move Notes")'
check "ctrl+shift+N new folder"           "key ctrl+shift+N; wait; key Enter"                    "folders: 11 "
check "undo New Folder"                   "key ctrl+shift+N; wait; key Enter; wait; key ctrl+Z"  "folders: 10 "
check "undo Delete Folder"                "click 100 258; wait; menu Delete; wait; key Enter; wait; key ctrl+Z" "folders: 10 "
check "ctrl+2 gallery"                    "click 320 290; key ctrl+2"                            "gallery=true"
check "ctrl+3 attachments"                "click 320 290; key ctrl+3"                            "attachments=true"
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
