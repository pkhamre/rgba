#!/bin/sh
# Build and package the x86_64 Linux desktop release without proprietary assets.
set -eu
cd "$(dirname "$0")/.."
if [ "$#" -ne 1 ]; then
    echo "usage: sh scripts/package-linux.sh vVERSION" >&2
    exit 2
fi
version=$1
case "$version" in
    ''|*[!a-zA-Z0-9._-]*) echo "invalid release tag: $version" >&2; exit 2 ;;
esac
case "$version" in
    v[0-9]*) ;;
    *) echo "release tag must start with v and a digit" >&2; exit 2 ;;
esac
target=x86_64-unknown-linux-musl
name="rgba-$version-$target"
output="$PWD/artifacts/release"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT HUP INT TERM

sh scripts/cargo.sh build --locked --release --target "$target" -p gba-desktop
binary="${CARGO_TARGET_DIR:-target}/$target/release/gba-desktop"
"$binary" --help
mkdir -p "$staging/$name" "$output"
cp "$binary" "$staging/$name/rgba"
chmod 755 "$staging/$name/rgba"
# Explicit allowlist: never package BIOS/ROM images, saves, or local build data.
cp README.md LICENSE "$staging/$name/"
cp -R docs LICENSES "$staging/$name/"
python3 - "$staging" "$output" "$name" <<'PY'
import hashlib
from pathlib import Path
import sys
import tarfile

staging, output, name = sys.argv[1:]
archive = Path(output) / f"{name}.tar.gz"
with tarfile.open(archive, "w:gz") as package:
    package.add(Path(staging) / name, arcname=name)
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(".gz.sha256").write_text(f"{digest}  {archive.name}\n")
PY
printf 'Release archive: %s/%s.tar.gz\n' "$output" "$name"
