#!/bin/bash
# Builds the Rust core as a static library and generates its Swift bindings.
# Called from the Xcode build phase and from scripts/build-app.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=15.0

cd "$ROOT/core"
cargo build -p nd-core --release --quiet

OUT="$ROOT/app/Generated"
mkdir -p "$OUT"
cargo run -p uniffi-bindgen --release --quiet -- generate \
    --library target/release/libnd_core.dylib \
    --language swift \
    --out-dir "$OUT"
mv -f "$OUT/nd_coreFFI.modulemap" "$OUT/module.modulemap"
