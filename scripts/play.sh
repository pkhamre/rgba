#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ "$#" -gt 0 ]; then
    case "$1" in
        --*) ;;
        *) rom=$1; shift; set -- --rom "$rom" "$@" ;;
    esac
fi
exec sh scripts/cargo.sh run --release -p gba-desktop -- \
    --bios bins/gba_bios.bin "$@"
