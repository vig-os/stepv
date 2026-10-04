#!/usr/bin/env bash
# S6 (vig-os/stepv#10): package target/macos/stepv.app as a DMG and, when
# Apple credentials are present, notarise and staple it.
#
# Distribution outside the App Store needs, from the Apple Developer account
# (NOT in this repo; the human provides them):
#   - a "Developer ID Application" certificate in the keychain, used by
#     build-macos-app.sh via STEPV_SIGN_IDENTITY;
#   - a notarytool keychain profile, created once with
#       xcrun notarytool store-credentials stepv-notary \
#         --apple-id <id> --team-id <TEAM> --password <app-specific password>
#     (or an App Store Connect API key: --key/--key-id/--issuer).
#
# Without a Developer ID signature, the DMG is still built (for local use or
# testing) but notarisation is skipped, loudly.
#
#   NOTARY_PROFILE  keychain profile name (default stepv-notary)
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
app="$root/target/macos/stepv.app"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
dmg="$root/target/macos/stepv-$version-$(uname -m).dmg"
profile=${NOTARY_PROFILE:-stepv-notary}
[ -d "$app" ] || { echo "package-macos: build first (just macos-app)" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -quiet -volname "stepv $version" -srcfolder "$stage" -format UDZO "$dmg"

authority=$(codesign -dvv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -1)
if [[ "$authority" != Developer\ ID\ Application* ]]; then
  echo "package-macos: $dmg built, NOT notarised: the app is signed by '${authority:-ad-hoc}'," >&2
  echo "  not a Developer ID. Set STEPV_SIGN_IDENTITY and rebuild to distribute." >&2
  exit 0
fi
codesign --force --sign "$authority" --timestamp "$dmg"
if ! xcrun notarytool history --keychain-profile "$profile" >/dev/null 2>&1; then
  echo "package-macos: no notarytool profile '$profile' — see the header of this script." >&2
  exit 1
fi
xcrun notarytool submit "$dmg" --keychain-profile "$profile" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
echo "package-macos: $dmg signed, notarised and stapled"
