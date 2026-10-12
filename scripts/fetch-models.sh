#!/bin/bash
# Downloads the face recognition model (OpenCV Zoo SFace, Apache-2.0) into
# ./models and refuses to keep it unless its SHA-256 matches.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIR="$ROOT/models"
FILE="$DIR/face_recognition_sface_2021dec.onnx"
SHA="0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79"
BASE="opencv/opencv_zoo/main/models/face_recognition_sface"

mkdir -p "$DIR"
if [ -f "$FILE" ] && [ "$(shasum -a 256 "$FILE" | cut -d' ' -f1)" = "$SHA" ]; then
    exit 0
fi
echo "→ Downloading the face model (38.7 MB) from github.com/$BASE"
curl -sSfL "https://media.githubusercontent.com/media/$BASE/face_recognition_sface_2021dec.onnx" -o "$FILE.part"
if [ "$(shasum -a 256 "$FILE.part" | cut -d' ' -f1)" != "$SHA" ]; then
    rm -f "$FILE.part"
    echo "✗ Face model checksum mismatch; not using it." >&2
    exit 1
fi
mv "$FILE.part" "$FILE"
curl -sSfL "https://raw.githubusercontent.com/$BASE/LICENSE" -o "$DIR/SFace-LICENSE.txt"
