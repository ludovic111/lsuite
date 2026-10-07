#!/usr/bin/env bash
# Builds lsuite.app (the launcher) for one macOS target and packages it for release:
#   scripts/bundle-macos.sh [aarch64-apple-darwin|x86_64-apple-darwin]     (default: this Mac)
# Writes to target/dist/:
#   lsuite.app (in target/dist/<triple>/)   the bundle: lsuite, lsuite-cli, lsuite-mcp
#   lsuite-macos-<arch>.dmg                 what people download (arch: arm64 or x86_64, the names
#                                          lsuite.xyz/launcher/download/macos-* looks for)
#   lsuite-macos-<arch>.app.tar.gz                the same app for in-app updaters
#
# Environment (all optional):
#   APPLE_SIGNING_IDENTITY       Developer ID identity; without it the build is ad-hoc signed
#   APPLE_API_KEY_PATH, APPLE_API_KEY, APPLE_API_ISSUER
#                                App Store Connect API key: notarize and staple (set by
#                                scripts/prepare-apple-signing.sh in the release workflow)
#   LSUITE_SKIP_BUILD=1           reuse the binaries already in target/<triple>/release
#   CARGO                        the cargo to run (default: cargo)
set -euo pipefail
cd "$(dirname "$0")/.."

triple=${1:-$(rustc -vV | sed -n 's/^host: //p')}
case "$triple" in
  aarch64-apple-darwin) arch=arm64 lipo_arch=arm64 ;;
  x86_64-apple-darwin) arch=x86_64 lipo_arch=x86_64 ;;
  *) echo "usage: $0 aarch64-apple-darwin|x86_64-apple-darwin" >&2; exit 1 ;;
esac
version=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)
[ -n "$version" ] || { echo "No version in Cargo.toml [workspace.package]" >&2; exit 1; }
identity=${APPLE_SIGNING_IDENTITY:--}
APPLE_API_KEY=${APPLE_API_KEY:-${APPLE_API_KEY_ID:-}}
cargo=${CARGO:-cargo}
resources=crates/lsuite-desktop/resources
dist=target/dist
app="$dist/$triple/lsuite.app"
bins=(lsuite lsuite-cli lsuite-mcp)

if [ "${LSUITE_SKIP_BUILD:-}" != 1 ]; then
  locked=()
  [ -n "${CI:-}" ] && locked=(--locked)
  packages=()
  for b in "${bins[@]}"; do packages+=(-p "$b"); done
  # GPUI needs 10.15.7; 11 like the apps.
  MACOSX_DEPLOYMENT_TARGET=11.0 "$cargo" build --release ${locked[@]+"${locked[@]}"} --target "$triple" "${packages[@]}"
fi

echo "Assembling $app ($version, $arch)"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for b in "${bins[@]}"; do
  src="target/$triple/release/$b"
  test -x "$src" || { echo "Missing $src; build it first." >&2; exit 1; }
  lipo "$src" -verify_arch "$lipo_arch"
  cp "$src" "$app/Contents/MacOS/$b"
done
cp "$resources/lsuite.icns" "$app/Contents/Resources/lsuite.icns"
cp LICENSE "$app/Contents/Resources/LICENSE.txt"
sed "s/@VERSION@/$version/g" "$resources/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" > /dev/null
printf 'APPL????' > "$app/Contents/PkgInfo"

# Hardened runtime everywhere (also ad-hoc, so local builds behave like releases); a secure
# timestamp only with a real identity. Helpers first, the bundle (and its main binary) last.
if [ "$identity" = - ]; then
  stamp=(--timestamp=none)
  echo "No APPLE_SIGNING_IDENTITY: ad-hoc signing (fine locally; Gatekeeper will refuse it elsewhere)."
else
  stamp=(--timestamp)
fi
sign() { # path identifier
  codesign --force --options runtime "${stamp[@]}" --identifier "$2" --sign "$identity" "$1"
}
for b in "${bins[@]}"; do
  [ "$b" = lsuite ] || sign "$app/Contents/MacOS/$b" "xyz.lsuite.launcher.${b#lsuite-}"
done
sign "$app" xyz.lsuite.launcher
codesign --verify --deep --strict --verbose=2 "$app"

notarize() { # path-to-submit
  local result status id
  result=$(xcrun notarytool submit "$1" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" --wait --timeout 45m --output-format json)
  echo "$result"
  status=$(printf '%s' "$result" | plutil -extract status raw -o - - 2> /dev/null || true)
  if [ "$status" != Accepted ]; then
    id=$(printf '%s' "$result" | plutil -extract id raw -o - - 2> /dev/null || true)
    [ -n "$id" ] && xcrun notarytool log "$id" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
      --issuer "$APPLE_API_ISSUER" || true
    echo "Notarization of $1 was not accepted: ${status:-no status}" >&2
    exit 1
  fi
}
can_notarize=0
if [ "$identity" != - ] && [ -n "${APPLE_API_KEY_PATH:-}" ] && [ -n "${APPLE_API_KEY:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ]; then
  can_notarize=1
fi
if [ "$can_notarize" = 1 ]; then
  echo "Notarizing lsuite.app"
  submission="$dist/lsuite-$arch-notarize.zip"
  rm -f "$submission"
  ditto -c -k --keepParent "$app" "$submission"
  notarize "$submission"
  rm -f "$submission"
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute --verbose=2 "$app"
fi

# The download: a disk image with the app and a link to /Applications.
dmg="$dist/lsuite-macos-$arch.dmg"
stage="$dist/$triple/dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
ditto "$app" "$stage/lsuite.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname lsuite -srcfolder "$stage" -fs HFS+ -format UDZO -imagekey zlib-level=9 -ov "$dmg" > /dev/null
rm -rf "$stage"
if [ "$identity" != - ]; then
  codesign --force --timestamp --sign "$identity" "$dmg"
fi
if [ "$can_notarize" = 1 ]; then
  echo "Notarizing $dmg"
  notarize "$dmg"
  xcrun stapler staple "$dmg"
fi

# The updater archive: one lsuite.app folder at the root, without AppleDouble files.
archive="$dist/lsuite-macos-$arch.app.tar.gz"
rm -f "$archive"
COPYFILE_DISABLE=1 tar --no-mac-metadata -C "$dist/$triple" -czf "$archive" lsuite.app

echo "Built lsuite $version for $triple:"
ls -lh "$dmg" "$archive"
