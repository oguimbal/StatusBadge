#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
source "$ROOT/bundle/release-config.sh"
OUTPUT_DIR="${DIST_DIR:-$ROOT/dist}"
APP="$OUTPUT_DIR/Status Badge.app"
DMG="$OUTPUT_DIR/StatusBadge.dmg"
test -d "$APP"
if [ -e "$DMG" ]; then
    echo "$DMG already exists; move it before building another DMG." >&2
    exit 1
fi
create-dmg --volname 'Status Badge' --window-pos 200 120 --window-size 660 400 \
    --icon-size 128 --icon 'Status Badge.app' 165 180 --app-drop-link 495 180 \
    --hide-extension 'Status Badge.app' --hdiutil-quiet "$DMG" "$APP"
codesign --force --sign "${CODESIGN_IDENTITY:?A Developer ID identity is required}" --timestamp "$DMG"
codesign --verify --strict "$DMG"
