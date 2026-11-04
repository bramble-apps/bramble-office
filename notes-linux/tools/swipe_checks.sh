#!/bin/bash
# Scripted checks of the note list's swipe rows: a row swiped open and shut again takes clicks,
# and after a note is deleted its neighbour (now where it was) opens at a click, nothing left
# open (on the demo library, nothing written to disk). Usage: tools/swipe_checks.sh [binary]
B=${1:-$(dirname "$0")/../target/debug/bramble-notes}
pass=0; fail=0
check() { # name, script, expected text in the NT_DUMP swipe and selection lines
    out=$(NT_DUMP=1 NT_SNAP=/dev/null NT_SIZE=1065x836 NT_NOTITLE=1 NT_SCRIPT="wait; $2; wait; wait" timeout -s KILL 90 "$B" 2>&1 | grep -E "^(swipe|selection):" | tr '\n' ' ')
    ok=1; for want in "${@:3}"; do [[ "$out" == *"$want"* ]] || ok=0; done
    if [ $ok = 1 ]; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1: wanted '${*:3}' in: $out"; fi
}
# demo's Bramble Research list (column 220..431): 33 Drawing at y 290, 31 URL Preview at y 346;
# swiped open, a row keeps a sliver at x 220..235, its Delete button at x 390
check "swiped open"                         "swipe 320 346 -12 12; wait; wait"                                  "open=true" "x=-186.0"
check "shut, then a click opens the note"   "swipe 320 346 -12 12; wait; wait; click 226 346; wait; wait; click 320 346" "open=false" "lead Some(\"31 URL Preview\")"
check "a click while it shuts opens it"     "swipe 320 346 -12 12; wait; wait; click 226 346; click 320 346"    "open=false" "menu=false" "lead Some(\"31 URL Preview\")"
check "Escape shuts, a click opens"         "swipe 320 346 -12 12; wait; wait; key Escape; wait; wait; click 320 346" "lead Some(\"31 URL Preview\")"
check "Delete button, the neighbour opens"  "swipe 320 290 -12 12; wait; wait; click 398 290; wait; wait; click 320 290" "row=None x=0.0 open=false" "deleted: 1" "lead Some(\"31 URL Preview\")"
check "full swipe, the neighbour opens"     "swipe 320 290 -30 20; wait; wait; wait; click 320 290"             "row=None x=0.0 open=false" "deleted: 1" "lead Some(\"31 URL Preview\")"
check "the neighbour swipes open by itself" "swipe 320 290 -30 20; wait; wait; wait; swipe 320 290 -12 12; wait; wait" "open=true" "x=-186.0" "deleted: 1"
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
