#!/usr/bin/env bash
set -euo pipefail

stage="${1:?provide the staged macOS release directory}"
: "${APPLE_SIGNING_IDENTITY:?}" "${APPLE_SIGNING_KEYCHAIN:?}" "${APPLE_TEAM_ID:?}"
: "${APPLE_API_KEY_PATH:?}" "${APPLE_API_KEY_ID:?}" "${APPLE_API_ISSUER:?}"
case "$APPLE_SIGNING_IDENTITY" in
  "Developer ID Application: "*) ;;
  *) echo "A Developer ID Application identity is required." >&2; exit 1 ;;
esac

app="$stage/Vapourfly.app"
for artifact in "$stage/vapourfly" "$app"; do
  codesign --force --timestamp --options runtime --sign "$APPLE_SIGNING_IDENTITY" \
    --keychain "$APPLE_SIGNING_KEYCHAIN" "$artifact"
  codesign --verify --deep --strict --verbose=2 "$artifact"
  codesign --display --verbose=4 "$artifact" 2>&1 | grep -Fx "TeamIdentifier=$APPLE_TEAM_ID"
done

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
# Submit both the app and the standalone CLI; neither tar nor a raw binary is a notary upload format.
ditto -c -k --keepParent "$stage" "$workdir/release.zip"
xcrun notarytool submit "$workdir/release.zip" \
  --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER" \
  --wait --timeout 30m --output-format json > "$workdir/result.json"
cat "$workdir/result.json"
jq -e '.status == "Accepted"' "$workdir/result.json"
xcrun stapler staple "$app"
xcrun stapler validate "$app"
spctl --assess --type execute --verbose=2 "$app"
# Standalone tools cannot hold a stapled ticket; check the CLI's ticket online.
codesign --verify --strict --check-notarization -R '=notarized' --verbose=2 "$stage/vapourfly"
