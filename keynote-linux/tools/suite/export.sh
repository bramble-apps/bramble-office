#!/bin/sh
# Copy the committed state of Keydeck, Values and Letters into the combined Bramble Office repo
# (a fresh copy without the development history, for sharing or publishing), then commit there.
#   tools/suite/export.sh <bramble-office dir> [commit message]
# Only committed files are copied, so nothing ignored (the private test files) can leak out.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
P=$(cd "$here/../../.." && pwd)
out=${1:?usage: export.sh <bramble-office dir> [message]}
msg=${2:-"Update from the development repos"}
mkdir -p "$out"
[ -d "$out/.git" ] || git -C "$out" init -q -b main
for r in keynote-linux numbers-linux pages-linux; do
  rm -rf "${out:?}/$r"
  mkdir -p "$out/$r"
  git -C "$P/$r" archive HEAD | tar -x -C "$out/$r"
done
cp "$here/README.md" "$out/README.md"
cp "$P/keynote-linux/LICENSE" "$out/LICENSE"
mkdir -p "$out/.github/workflows"
cp "$here/build.yml" "$out/.github/workflows/build.yml"
printf 'target/\n/*/testdata/\n/*/testdata-private/\ndist/\n' > "$out/.gitignore"
git -C "$out" add -A
if git -C "$out" diff --cached --quiet; then
  echo "nothing changed"
else
  # (the public repo shows the GitHub no-reply address, not a personal one)
  git -C "$out" -c user.name="Stephan D." -c user.email="180196315+StephanRD@users.noreply.github.com" commit -q -m "$msg"
  git -C "$out" log --oneline -1
fi
