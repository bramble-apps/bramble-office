#!/bin/sh
# Build (optimised) and run Keynote for Linux. Usage: ./run.sh [file.key]
cd "$(dirname "$0")" && exec cargo run --release -p kn-app -- "$@"
