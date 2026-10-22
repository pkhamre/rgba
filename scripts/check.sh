#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
sh scripts/cargo.sh fmt --all -- --check
sh scripts/cargo.sh clippy --workspace --all-targets -- -D warnings
sh scripts/cargo.sh test --workspace
python3 -m unittest discover -s scripts/tests
