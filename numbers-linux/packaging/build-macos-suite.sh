#!/bin/bash
# Build and install the suite on a Mac: Keydeck.app (keynote-linux/packaging/build-macos.sh)
# and Values.app (packaging/build-macos.sh). keynote-linux sits beside numbers-linux, as the
# crates' paths expect.
#   packaging/build-macos-suite.sh [--install]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
"$ROOT/../keynote-linux/packaging/build-macos.sh" "$@"
"$ROOT/packaging/build-macos.sh" "$@"
