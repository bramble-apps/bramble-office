#!/bin/bash
# builds are shared with other worktrees: touch our sources so cargo never trusts a foreign rlib
cd /home/stephandalton/CascadeProjects/st-s2-sculpt/logic-linux
export CARGO_TARGET_DIR=/home/stephandalton/CascadeProjects/st-synths2/logic-linux/target CARGO_INCREMENTAL=0
touch crates/lg-synth/src/lib.rs crates/lg-synth/src/sculpture*.rs
timeout 2400 cargo "$@" 2>&1
