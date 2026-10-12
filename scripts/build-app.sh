#!/bin/bash
# One command: tests the core, builds the app, and puts a self-contained
# Folio.app in ./dist (Rust core statically linked, icon included).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
cd "$ROOT"

echo "→ Checking the face model"
./scripts/fetch-models.sh

echo "→ Testing the core"
(cd core && cargo test -p nd-core --quiet)

echo "→ Building the Rust core and Swift bindings"
./scripts/build-core.sh

echo "→ Generating the Xcode project"
xcodegen generate --quiet

echo "→ Building Folio.app (Release)"
xcodebuild -project Folio.xcodeproj -scheme Folio -configuration Release \
    -derivedDataPath build/DerivedData build -quiet

mkdir -p dist
rm -rf dist/Folio.app
cp -R build/DerivedData/Build/Products/Release/Folio.app dist/
echo "→ Done: dist/Folio.app ($(du -sh dist/Folio.app | cut -f1))"
