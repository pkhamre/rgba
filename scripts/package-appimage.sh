#!/bin/sh
# Build and package the x86_64 Linux desktop application as an AppImage.
set -eu
cd "$(dirname "$0")/.."
if [ "$#" -ne 1 ]; then
    echo "usage: sh scripts/package-appimage.sh vVERSION" >&2
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
appimagetool=${APPIMAGETOOL:-appimagetool}
if ! command -v "$appimagetool" >/dev/null 2>&1 && [ ! -x "$appimagetool" ]; then
    echo "appimagetool not found; install it or set APPIMAGETOOL to its path" >&2
    exit 127
fi
target=x86_64-unknown-linux-musl
name="rgba-$version-$target"
output="$PWD/artifacts/release"
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT HUP INT TERM

sh scripts/cargo.sh build --locked --release --target "$target" -p gba-desktop
binary="${CARGO_TARGET_DIR:-target}/$target/release/gba-desktop"
"$binary" --help
appdir="$staging/$name.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/applications" "$appdir/usr/share/icons/hicolor/scalable/apps" "$output"
cp "$binary" "$appdir/usr/bin/rgba"
chmod 755 "$appdir/usr/bin/rgba"
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
HERE=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec "$HERE/usr/bin/rgba" "$@"
EOF
chmod 755 "$appdir/AppRun"
cat > "$appdir/rgba.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=rgba
Comment=Game Boy Advance emulator
Exec=rgba
Icon=rgba
Categories=Game;Emulator;
Terminal=false
EOF
cp "$appdir/rgba.desktop" "$appdir/usr/share/applications/rgba.desktop"
cat > "$appdir/usr/share/icons/hicolor/scalable/apps/rgba.svg" <<'EOF'
<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128" viewBox="0 0 128 128">
<rect x="8" y="8" width="112" height="112" rx="20" fill="#5965a8"/>
<text x="64" y="78" fill="white" font-family="sans-serif" font-size="42" font-weight="bold" text-anchor="middle">rgba</text>
</svg>
EOF
cp "$appdir/usr/share/icons/hicolor/scalable/apps/rgba.svg" "$appdir/rgba.svg"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$appimagetool" "$appdir" "$output/$name.AppImage"
chmod 755 "$output/$name.AppImage"
python3 - "$output/$name.AppImage" <<'PY'
import hashlib
from pathlib import Path
import sys

image = Path(sys.argv[1])
digest = hashlib.sha256(image.read_bytes()).hexdigest()
image.with_suffix(".AppImage.sha256").write_text(f"{digest}  {image.name}\n")
PY
printf 'AppImage: %s/%s.AppImage\n' "$output" "$name"
