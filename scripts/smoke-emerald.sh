#!/bin/sh
# Bounded ROM/BIOS/graphics/keypad smoke run; never loads the user's normal save.
set -eu
cd "$(dirname "$0")/.."
mkdir -p artifacts
output_dir="artifacts/emerald-smoke-$$"
mkdir "$output_dir"
output="$output_dir/screenshot.ppm"
{
    printf 'run 2000 0\n'
    for event in 1 2 3; do
        printf 'run 10 8\nrun 390 0\n'
    done
    for event in 1 2; do
        printf 'run 10 1\nrun 390 0\n'
    done
    printf 'shot %s\nquit\n' "$output"
} | sh scripts/cargo.sh run --release -p gba-host --example scripted_session -- \
    bins/gba_bios.bin \
    "bins/Pokemon - Emerald Version (USA, Europe).gba" \
    "$output_dir/disposable.sav"
printf 'Screenshot: %s\n' "$output"
