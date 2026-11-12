#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
source "$ROOT/bundle/release-config.sh"
OUTPUT_DIR="${DIST_DIR:-$ROOT/dist}"
python3 bundle/verify-version.py
export MACOSX_DEPLOYMENT_TARGET=13.0
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
PROFILE="${BUNDLE_PROFILE:-release}"
BUILD_FLAGS=(--locked -p statusbadge)
if [ "$PROFILE" = release ]; then BUILD_FLAGS+=(--release); fi
if [ "$PROFILE" != release ] && [ "$PROFILE" != debug ]; then
    echo 'BUNDLE_PROFILE must be release or debug' >&2
    exit 1
fi
STAGE="$(mktemp -d "$ROOT/dist-stage.XXXXXX")"
trap 'rm -rf "$STAGE"' EXIT
APP="$STAGE/Status Badge.app"
mkdir -p "$APP/Contents/MacOS"
if [ "${BUNDLE_UNIVERSAL:-0}" = 1 ]; then
    cargo build "${BUILD_FLAGS[@]}" --target aarch64-apple-darwin
    cargo build "${BUILD_FLAGS[@]}" --target x86_64-apple-darwin
    lipo -create "$TARGET_DIR/aarch64-apple-darwin/$PROFILE/statusbadge" \
        "$TARGET_DIR/x86_64-apple-darwin/$PROFILE/statusbadge" -output "$APP/Contents/MacOS/statusbadge"
else
    cargo build "${BUILD_FLAGS[@]}"
    cp "$TARGET_DIR/$PROFILE/statusbadge" "$APP/Contents/MacOS/statusbadge"
fi
cp bundle/Info.plist "$APP/Contents/Info.plist"
mkdir -p "$APP/Contents/Resources"
cp bundle/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
IDENTITY="${CODESIGN_IDENTITY:-}"
if [ -z "$IDENTITY" ]; then
    IDENTITY="$(security find-identity -v -p codesigning | awk -F'"' '/Developer ID Application/ {print $2; exit}')"
fi
IDENTITY="${IDENTITY:--}"
FLAGS=(--force --sign "$IDENTITY")
if [[ "$IDENTITY" == *'Developer ID Application'* ]]; then FLAGS+=(--options runtime --timestamp); fi
codesign "${FLAGS[@]}" "$APP"
codesign --verify --strict "$APP"
mkdir -p "$OUTPUT_DIR"
# Only replaces this repository's generated local build, never a Spaces object.
if [ -e "$OUTPUT_DIR/Status Badge.app" ]; then
    mv "$OUTPUT_DIR/Status Badge.app" "$(mktemp -d "$OUTPUT_DIR/previous-build.XXXXXX")/Status Badge.app"
fi
mv "$APP" "$OUTPUT_DIR/Status Badge.app"
echo "Built $OUTPUT_DIR/Status Badge.app"
