#!/bin/sh
# Render a document with OnlyOffice's own converter (x2t), headless:
#   tools/oo-render.sh in.docx out.pdf      (also .pptx, .xlsx, .pages, .key, …; out .pdf/.docx/.pptx)
# Uses the OnlyOffice desktop snap's converter and the fonts list it generated on first run.
set -e
in=$(realpath "$1"); out=$(realpath -m "$2")
X=/snap/onlyoffice-desktopeditors/current/opt/onlyoffice/desktopeditors/converter
F=$(ls -d "$HOME"/snap/onlyoffice-desktopeditors/*/.local/share/onlyoffice/desktopeditors/data/fonts 2>/dev/null | tail -1)
[ -x "$X/x2t" ] || { echo "OnlyOffice desktop editors (snap) not found" >&2; exit 2; }
[ -n "$F" ] || { echo "start OnlyOffice once so it lists the fonts" >&2; exit 2; }
case "$out" in
  *.pdf) fmt=513 ;; *.docx) fmt=65 ;; *.pptx) fmt=129 ;; *.xlsx) fmt=257 ;; *.odt) fmt=67 ;;
  *) echo "unknown output type: $out" >&2; exit 2 ;;
esac
tmp=$(mktemp -d)
cat > "$tmp/task.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<TaskQueueDataConvert><m_sFileFrom>$in</m_sFileFrom><m_sFileTo>$out</m_sFileTo><m_nFormatTo>$fmt</m_nFormatTo><m_sTempDir>$tmp</m_sTempDir><m_sFontDir>$F</m_sFontDir><m_sAllFontsPath>$F/AllFonts.js</m_sAllFontsPath><m_bIsNoBase64>true</m_bIsNoBase64></TaskQueueDataConvert>
XML
cd "$X" && LD_LIBRARY_PATH="$X" ./x2t "$tmp/task.xml"
rc=$?
rm -rf "$tmp"
[ -s "$out" ] || { echo "OnlyOffice produced nothing (code $rc)" >&2; exit 1; }
