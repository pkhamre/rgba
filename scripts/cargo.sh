#!/bin/sh
# Use the isolated toolchain when the host has no Rust/C toolchain.
set -eu
cd "$(dirname "$0")/.."
if command -v cargo >/dev/null 2>&1; then
    exec cargo "$@"
fi
export CARGO_HOME="$PWD/.tools/cargo"
export RUSTUP_HOME="$PWD/.tools/rustup"
export PATH="$CARGO_HOME/bin:$PATH"
export CARGO_BUILD_TARGET=x86_64-unknown-linux-musl
exec cargo "$@"
