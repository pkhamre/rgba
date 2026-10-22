#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
if [ -n "${XDG_DATA_HOME:-}" ]; then
    case "$XDG_DATA_HOME" in
        /*) data_home=$XDG_DATA_HOME ;;
        *) data_home=${HOME:?HOME must be set when XDG_DATA_HOME is relative}/.local/share ;;
    esac
else
    data_home=${HOME:?HOME must be set}/.local/share
fi
bios=${RGBA_BIOS:-"$data_home/rgba/bios.bin"}
if [ -z "${RGBA_BIOS:-}" ] && [ ! -f "$bios" ] && [ -f bins/gba_bios.bin ]; then
    bios=bins/gba_bios.bin
fi
if [ "$#" -gt 0 ]; then
    case "$1" in
        --*) ;;
        *) rom=$1; shift; set -- --rom "$rom" "$@" ;;
    esac
fi
has_bios_option=0
for option in "$@"; do
    if [ "$option" = --bios ]; then
        has_bios_option=1
        break
    fi
done
if [ "$has_bios_option" -eq 0 ]; then
    set -- --bios "$bios" "$@"
fi
exec sh scripts/cargo.sh run --release -p gba-desktop -- "$@"
