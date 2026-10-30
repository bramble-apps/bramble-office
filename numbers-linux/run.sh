#!/bin/sh
# Build (release, for a smooth 120 Hz canvas) and start Values; pass a .numbers / .csv file to open it.
cd "$(dirname "$0")" && cargo build --release -p nm-app && exec ./target/release/values "$@"
