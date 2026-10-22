#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
rom="bins/Pokemon - Emerald Version (USA, Europe).gba"
if [ "$#" -gt 0 ]; then
    case "$1" in
        --*) ;;
        *) rom="$1"; shift ;;
    esac
fi
exec sh scripts/cargo.sh run --release -p gba-desktop -- \
    --bios bins/gba_bios.bin --rom "$rom" "$@"
