#!/bin/bash

# Set "chatgpt.cliExecutable": "/Users/<USERNAME>/code/dex/scripts/debug-dex.sh" in VSCode settings to always get the 
# latest dex-rs binary when debugging Dex Extension.


set -euo pipefail

DEX_RS_DIR=$(realpath "$(dirname "$0")/../dex-rs")
(cd "$DEX_RS_DIR" && cargo run --quiet --bin dex -- "$@")
