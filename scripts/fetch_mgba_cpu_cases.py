#!/usr/bin/env python3
"""Fetch pinned, MIT-licensed mGBA CPU baselines and lower isolated test cases."""

import hashlib
import json
from pathlib import Path
import urllib.request

import mgba_cpu_cases

REVISION = "e6942030d25ffe3ba76c72b73a86da073ec857cc"
BASE_URL = f"https://raw.githubusercontent.com/mgba-emu/suite/{REVISION}"
DESTINATION = Path(__file__).resolve().parents[1] / "tests/assets/mgba-suite-cpu"
BLOBS = {
    "LICENSE": "81df81f3f0e97941e37f4154dac50eef4a271981",
    "src/carry.c": "8b545dcc9bbe7675246c431779d4a0b564709aae",
    "src/shifter.c": "e63abb915c09598d5766b9705b677c5e8fc9ba2f",
    "src/shifter-impl.s": "547467832318548538ac220ea16c692e47d7c58f",
    "src/multiply-long.c": "d9f11e51603b63a7a3a71e293a261353b18e8f7d",
}


def fetch(name, expected_blob):
    path = DESTINATION / name
    if path.exists():
        data = path.read_bytes()
    else:
        with urllib.request.urlopen(f"{BASE_URL}/{name}", timeout=45) as response:
            data = response.read()
    blob = hashlib.sha1(f"blob {len(data)}\0".encode("ascii") + data).hexdigest()
    if blob != expected_blob:
        raise ValueError(f"Pinned Git blob mismatch for {name}; remove stale/corrupt cached data")
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(path.name + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(path)
    return data


def main():
    sources = {}
    manifest = {"repository": "https://github.com/mgba-emu/suite",
                "revision": REVISION, "source_files": {}}
    for name, blob in BLOBS.items():
        data = fetch(name, blob)
        manifest["source_files"][name] = {
            "git_blob": blob, "sha256": hashlib.sha256(data).hexdigest(),
        }
        sources[name] = data.decode("utf-8")
    cases = mgba_cpu_cases.build_cases(sources).encode("utf-8")
    path = DESTINATION / "cases.tsv"
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_bytes(cases)
    temporary.replace(path)
    manifest["cases_sha256"] = hashlib.sha256(cases).hexdigest()
    adapter = Path(mgba_cpu_cases.__file__).read_bytes()
    manifest["adapter_sha256"] = hashlib.sha256(adapter).hexdigest()
    manifest["instruction_address"] = f"{mgba_cpu_cases.INSTRUCTION_ADDRESS:08x}"
    manifest["coverage"] = {"carry": 93, "shifter": 70, "multiply-long": 72}
    (DESTINATION / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"Verified {len(BLOBS)} upstream files; generated 235 isolated CPU cases at {path}")
    print("Upstream LICENSE retained. No downloaded source or host code was executed.")


if __name__ == "__main__":
    main()
