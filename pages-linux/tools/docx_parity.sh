#!/bin/sh
# Lay out Word documents in Letters and in OnlyOffice and compare them page by page:
#   tools/docx_parity.sh a.docx [b.docx …]
# For each file, two checks: Letters' layout against OnlyOffice's, and OnlyOffice showing the
# file as Letters saves it again against OnlyOffice showing the original. Each gives both page
# counts, then per page the lines matched and how many sit more than 1.5 pt apart
# (tools/ink_lines.py at 72 dpi; pages that agree are not listed). Renders go to
# target/parity/<name>/.
set -e
root=$(cd "$(dirname "$0")/.." && pwd)
cargo build -q --release -p pg-core --example snap --example rtcheck --manifest-path "$root/Cargo.toml"
snap="$root/target/release/examples/snap"
rtcheck="$root/target/release/examples/rtcheck"
# compare two sets of page renders: compare <label> <a prefix> <b prefix>
compare() {
  na=$(ls "$2"-*.png | wc -l); nb=$(ls "$3"-*.png | wc -l)
  echo "  $1: $na / $nb pages$( [ "$na" = "$nb" ] || echo '  << PAGE COUNT DIFFERS')"
  n=1
  for a in $(ls "$2"-*.png | sort -V); do
    b=$(ls "$3"-*.png | sort -V | sed -n "${n}p")
    [ -n "$b" ] || break
    python3 -I "$root/tools/ink_lines.py" "$a" "$b" | awk -v n=$n 'NR>1 && NF==7 {c++; if ($3>1.5||$3<-1.5) o++} NR>1 && NF!=7 {u++} END {if (o+u>0) printf "    page %d: %d lines, %d off by >1.5pt, %d unmatched\n", n, c, o, u}'
    n=$((n+1))
  done
}
for f in "$@"; do
  name=$(basename "$f" .docx)
  out="$root/target/parity/$name"
  rm -rf "$out" && mkdir -p "$out"
  cp "$f" "$out/in.docx"
  "$root/tools/oo-render.sh" "$out/in.docx" "$out/oo.pdf" >/dev/null 2>&1 || { echo "$name: OnlyOffice failed"; continue; }
  pdftoppm -r 72 -png "$out/oo.pdf" "$out/oo"
  "$snap" "$out/in.docx" "$out/cs" 1 2>/dev/null
  echo "== $name"
  compare "Letters vs OnlyOffice" "$out/oo" "$out/cs"
  mkdir -p "$out/rt"
  "$rtcheck" "$out/rt" "$out/in.docx" >/dev/null 2>&1 || true
  if "$root/tools/oo-render.sh" "$out/rt/in.rt.docx" "$out/rt.pdf" >/dev/null 2>&1; then
    pdftoppm -r 72 -png "$out/rt.pdf" "$out/rtoo"
    compare "re-saved vs original, in OnlyOffice" "$out/oo" "$out/rtoo"
  else
    echo "  re-save: OnlyOffice could not open it"
  fi
done
