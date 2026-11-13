#!/bin/sh
# Fetches the two text-recognition models regnmed-ocr loads at start
# (docs/bilagstolkning.md) into target/ocr-models, where the Dockerfile
# copies them from. Pinned by SHA-256: the files come from the ocrs
# project's bucket, and a model that changes under us would change what
# every receipt suggests -- so a new model is a commit, never a surprise.
#
# Not in git (12 MB of weights), and not needed to build or test anything
# else: regnmed-ocr's model test skips without OCR_MODELS_DIR.
set -eu
DIR=${1:-target/ocr-models}
BASE=https://ocrs-models.s3-accelerate.amazonaws.com
mkdir -p "$DIR"
fetch() { # name sha256
    if [ -f "$DIR/$1" ] && echo "$2  $DIR/$1" | shasum -a 256 -c - >/dev/null 2>&1; then
        echo "ok    $1 (cached)"
        return
    fi
    curl -sSfL -o "$DIR/$1.part" "$BASE/$1"
    echo "$2  $DIR/$1.part" | shasum -a 256 -c - >/dev/null || {
        echo "FAIL  $1: checksum mismatch -- the published model changed" >&2
        rm -f "$DIR/$1.part"
        exit 1
    }
    mv "$DIR/$1.part" "$DIR/$1"
    echo "ok    $1"
}
fetch text-detection.onnx a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe
fetch text-recognition.onnx 86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5
