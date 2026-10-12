#!/bin/bash
# Publishes a new version:  scripts/release.sh 0.2.0
# Bumps the version, builds the DMG, creates the GitHub release, and points
# the Homebrew cask (Hong-Iron/homebrew-tap) at it.
set -euo pipefail

VERSION="${1:?usage: scripts/release.sh <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ -n "$(git status --porcelain)" ]; then
    echo "Commit or stash your changes first." >&2
    exit 1
fi

BUILD=$(( $(sed -nE 's/.*CFBundleVersion: "([0-9]+)".*/\1/p' project.yml) + 1 ))
sed -i '' -E "s/(CFBundleShortVersionString: )\"[^\"]+\"/\1\"$VERSION\"/" project.yml
sed -i '' -E "s/(CFBundleVersion: )\"[0-9]+\"/\1\"$BUILD\"/" project.yml
sed -i '' -E "s/^version = \"[^\"]+\"/version = \"$VERSION\"/" core/Cargo.toml

./scripts/package-dmg.sh
DMG="dist/Folio-$VERSION.dmg"
SHA=$(cut -d' ' -f1 "$DMG.sha256")

git commit -qam "Folio $VERSION"
git tag "v$VERSION"
git push -q origin HEAD "v$VERSION"
gh release create "v$VERSION" "$DMG" "$DMG.sha256" --title "Folio $VERSION" --generate-notes

TAP=$(mktemp -d)
trap 'rm -rf "$TAP"' EXIT
gh repo clone Hong-Iron/homebrew-tap "$TAP" -- -q
sed -i '' -E "s/^  version \".*\"/  version \"$VERSION\"/; s/^  sha256 \".*\"/  sha256 \"$SHA\"/" "$TAP/Casks/folio.rb"
git -C "$TAP" commit -qam "folio $VERSION"
git -C "$TAP" push -q
echo "→ Released Folio $VERSION"
