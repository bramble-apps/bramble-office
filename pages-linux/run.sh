#!/bin/sh
# Build (release, for a smooth 120 Hz canvas) and start Letters; pass a .pages / .docx / .rtf / .txt / .md file to open it.
cd "$(dirname "$0")" && cargo build --release -p pg-app && exec ./target/release/letters "$@"
