#!/bin/sh
# Record an unpaced core workload; never elevate privileges or change sysctls.
set -eu
if [ "$#" -lt 2 ]; then
    echo 'Usage: sh scripts/profile-headless.sh NEW_OUTPUT_DIR --bios PATH --rom PATH --frames COUNT [benchmark options]' >&2
    exit 2
fi
output=$1
shift
case "$output" in
    /*) ;;
    *) output="$PWD/$output" ;;
esac
command -v perf >/dev/null 2>&1 || { echo 'Linux perf is required.' >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo 'Python 3 is required.' >&2; exit 1; }
# Require a new directory to avoid overwriting previous measurements or inputs.
mkdir -m 700 -- "$output"
root=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
# Cargo JSON provides the actual executable, including custom target directories.
sh "$root/scripts/cargo.sh" build --profile profiling -p gba-headless --message-format=json > "$output/build.json"
binary=$(python3 - "$output/build.json" <<'PY'
import json
import sys

executable = None
with open(sys.argv[1], encoding="utf-8") as source:
    for line in source:
        message = json.loads(line)
        if (message.get("reason") == "compiler-artifact"
                and message.get("target", {}).get("name") == "gba-headless"
                and message.get("executable")):
            executable = message["executable"]
if executable is None:
    sys.exit("Cargo did not report the gba-headless executable")
print(executable)
PY
)
# No pipeline: preserve perf/the workload's failure status. DWARF unwinding does
# not require changing release code generation to force frame pointers.
if ! perf record -e cycles:u -F 199 --call-graph dwarf -o "$output/perf.data" -- \
    "$binary" --benchmark "$@" > "$output/benchmark.log" 2>&1; then
    echo "Profiling failed; inspect $output/benchmark.log. Check perf permissions and event availability; no system settings were changed." >&2
    exit 1
fi
perf report --stdio -i "$output/perf.data" > "$output/report.txt"
printf 'Benchmark log: %s/benchmark.log\nProfile report: %s/report.txt\n' "$output" "$output"
