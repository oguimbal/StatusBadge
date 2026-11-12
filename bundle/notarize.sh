#!/usr/bin/env bash
# Developer ID -> notarytool -> staple, for a signed .app or .dmg. Waits for Apple's answer.
# Credentials:
#   - release workflow: NOTARY_APPLE_ID, NOTARY_TEAM_ID and NOTARY_PASSWORD (repository secrets);
#   - local build: the Keychain profile $NOTARYTOOL_PROFILE, created once with
#     xcrun notarytool store-credentials <profile> --apple-id <id> --team-id <team>
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/release-config.sh"
ARTIFACT="${1:?Expected a signed .app or .dmg}"
if [ -n "${NOTARY_APPLE_ID:-}" ] && [ -n "${NOTARY_TEAM_ID:-}" ] && [ -n "${NOTARY_PASSWORD:-}" ]; then
    AUTH=(--apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD")
else
    AUTH=(--keychain-profile "$NOTARYTOOL_PROFILE")
fi
case "$ARTIFACT" in
    *.app)
        TEMP_DIR="$(mktemp -d)"
        trap 'rm -rf "$TEMP_DIR"' EXIT
        ditto -c -k --keepParent "$ARTIFACT" "$TEMP_DIR/application.zip"
        xcrun notarytool submit "$TEMP_DIR/application.zip" "${AUTH[@]}" --wait
        ;;
    *.dmg) xcrun notarytool submit "$ARTIFACT" "${AUTH[@]}" --wait ;;
    *) echo 'Expected .app or .dmg' >&2; exit 2 ;;
esac
xcrun stapler staple "$ARTIFACT"
xcrun stapler validate "$ARTIFACT"
