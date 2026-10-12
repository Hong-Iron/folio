#!/bin/bash
# Builds Folio and wraps it in a drag-to-Applications disk image:
#   dist/Folio-<version>.dmg  and its SHA-256 next to it.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
./scripts/build-app.sh

VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" dist/Folio.app/Contents/Info.plist)
DMG="dist/Folio-$VERSION.dmg"
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT

cp -R dist/Folio.app "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "Folio $VERSION" -srcfolder "$STAGE" -fs HFS+ -format ULFO "$DMG"
shasum -a 256 "$DMG" | tee "$DMG.sha256"
echo "→ $DMG ($(du -h "$DMG" | cut -f1))"
