#!/bin/sh
# Builds Oni.app (and Oni-macos.zip) into target/package/ (#105).
#   packaging/macos/build-app.sh            this Mac's architecture
#   packaging/macos/build-app.sh --universal  arm64 + x86_64 (both targets
#                                             installed: rustup target add …)
# The bundle is signed ad hoc only: unsigned by a developer ID and not
# notarized, macOS asks the player to open it from the context menu the
# first time.
set -eu
cd "$(dirname "$0")/../.."
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=target/package
app=$out/Oni.app
rm -rf "$app" "$out/Oni-macos.zip"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

if [ "${1:-}" = "--universal" ]; then
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --release -p oni --target "$t"
    done
    lipo -create -output "$app/Contents/MacOS/oni" \
        target/aarch64-apple-darwin/release/oni target/x86_64-apple-darwin/release/oni
else
    cargo build --release -p oni
    cp target/release/oni "$app/Contents/MacOS/oni"
fi

sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

# The icon: every size from the one picture.
iconset=$out/Oni.iconset
rm -rf "$iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
    sips -z $s $s packaging/icons/oni-512.png --out "$iconset/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    [ $d -le 512 ] && sips -z $d $d packaging/icons/oni-512.png --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
cp packaging/icons/oni-512.png "$iconset/icon_512x512@2x.png"
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Oni.icns"
rm -rf "$iconset"

# The licences that come with it.
mkdir -p "$app/Contents/Resources/licenses"
cp packaging/NOTICE.md LICENSE "$app/Contents/Resources/licenses/"
cp crates/oni/assets/launcher/OFL.txt "$app/Contents/Resources/licenses/Rajdhani-OFL.txt"

codesign --force --deep --sign - "$app"
(cd "$out" && ditto -c -k --keepParent Oni.app Oni-macos.zip)
echo "built $app and $out/Oni-macos.zip ($version)"
