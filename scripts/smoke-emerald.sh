#!/bin/sh
# Bounded ROM/BIOS/graphics/keypad smoke run; does not prove full-game accuracy.
set -eu
cd "$(dirname "$0")/.."
mkdir -p artifacts
output="artifacts/emerald-smoke-$$.ppm"
sh scripts/cargo.sh run --release -p gba-headless -- \
    --bios bins/gba_bios.bin \
    --rom "bins/Pokemon - Emerald Version (USA, Europe).gba" \
    --frames 4000 \
    --input 2000:start --input 2010:none \
    --input 2400:start --input 2410:none \
    --input 2800:start --input 2810:none \
    --input 3200:a --input 3210:none \
    --input 3600:a --input 3610:none \
    --screenshot "$output"
printf 'Screenshot: %s\n' "$output"
