#!/usr/bin/env python3
"""Fetch pinned, MIT-licensed jsmolka GBA CPU and memory test ROMs."""

import hashlib
from pathlib import Path
import urllib.request

REVISION = "a7113b67e63f83a9b321696ddd7042ccfad6c881"
BASE_URL = f"https://raw.githubusercontent.com/jsmolka/gba-tests/{REVISION}"
DESTINATION = Path(__file__).resolve().parents[1] / "tests/assets/jsmolka-gba-tests"
FILES = {
    "LICENSE": ("74a3e51b7f29f69d36ea2bcb698cad4841350dbf", "b59a9ed235d76752c977f04116bc72155a14af48176bdbf50d7e5c29fca35aa7"),
    "arm/arm.gba": ("dd0c023da55cc9a62afc90bc6917ceca689d1e29", "77ee88662552bdc885c1080c0172ff119d54db791bd73b21808cf1ff1fe5b40e"),
    "thumb/thumb.gba": ("47e50db6920d5a996051d0902365e5cdf7b166b6", "b5cb2291df4ab314b31c598acd9bff2ccfa0b38efff29daadfe97422ce369b67"),
    "memory/memory.gba": ("d87d0752563b27e37fa46e9b9d4b8ea7be412a8a", "21024fb6aae6343f5f0466dd54e3149de1fbeb23f78e7d85a015c983684d2f87"),
}


def fetch(name, expected_blob, expected_sha256):
    path = DESTINATION / name
    if path.exists():
        data = path.read_bytes()
    else:
        with urllib.request.urlopen(f"{BASE_URL}/{name}", timeout=45) as response:
            data = response.read()
    blob = hashlib.sha1(f"blob {len(data)}\0".encode("ascii") + data).hexdigest()
    digest = hashlib.sha256(data).hexdigest()
    if blob != expected_blob or digest != expected_sha256:
        raise ValueError(f"Pinned checksum mismatch for {name}; remove corrupt cached data")
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_name(path.name + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(path)
    return len(data)


def main():
    for name, (blob, digest) in FILES.items():
        size = fetch(name, blob, digest)
        print(f"Verified {name}: {size} bytes")
    print(f"Pinned revision: {REVISION}; upstream MIT license retained under {DESTINATION}")
    print("Test ROMs remain ignored and are not committed.")


if __name__ == "__main__":
    main()
