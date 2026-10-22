#!/usr/bin/env python3
"""Fetch pinned, MIT-licensed reference CPU vectors; never execute downloaded code."""

import concurrent.futures
import hashlib
import json
from pathlib import Path
import urllib.request

COMMIT = "e3097d88d428752736b949d94c094d5da50f0db6"
BASE = f"https://raw.githubusercontent.com/SingleStepTests/ARM7TDMI/{COMMIT}"
DESTINATION = Path(__file__).resolve().parents[1] / "tests/assets/arm7tdmi"
# Include absent-coprocessor and undefined encodings: the core must vector them.


def fetch(asset):
    name, expected_blob = asset
    path = DESTINATION / name
    if path.exists():
        data = path.read_bytes()
    else:
        with urllib.request.urlopen(f"{BASE}/{name}", timeout=90) as response:
            data = response.read()
    header = f"blob {len(data)}\0".encode("ascii")
    actual_blob = hashlib.sha1(header + data).hexdigest()
    if actual_blob != expected_blob:
        raise ValueError(f"Pinned Git blob mismatch for {name}; remove a stale cached file")
    if not path.exists():
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(path)
    return name, {"bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def main():
    DESTINATION.mkdir(parents=True, exist_ok=True)
    api = f"https://api.github.com/repos/SingleStepTests/ARM7TDMI/git/trees/{COMMIT}?recursive=1"
    with urllib.request.urlopen(api, timeout=30) as response:
        tree = json.load(response)
    if tree.get("truncated"):
        raise ValueError("GitHub returned an incomplete source tree")
    assets = []
    for entry in tree["tree"]:
        name = entry["path"]
        vector = name.startswith("v1/") and name.endswith(".json.bin")
        if name in {"LICENSE", "README.md"} or vector:
            assets.append((name, entry["sha"]))
    (DESTINATION / "v1").mkdir(exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as executor:
        manifest = dict(executor.map(fetch, assets))
    (DESTINATION / "manifest.json").write_text(
        json.dumps({"commit": COMMIT, "files": manifest}, indent=2) + "\n"
    )
    print(f"Verified {len(assets) - 2} vector groups in {DESTINATION}")
    print("Data and its upstream MIT license are local-only; see docs/TESTING.md.")


if __name__ == "__main__":
    main()
