#!/usr/bin/env bash
# Sets the GitHub secrets an lsuite app's release workflow uses to sign and notarize on macOS
# (the six names ryolune and kimchi share; see STANDARD.md, "Automatic updates").
#
#   APPLE_API_ISSUER=<issuer id> scripts/set-apple-secrets.sh <owner/repo> <AuthKey_XXXX.p8> [developer-id.p12]
#   scripts/set-apple-secrets.sh <owner/repo> - developer-id.p12      # certificate only, key kept
#
# - The .p8 is an App Store Connect API key (Users and Access > Integrations > App Store Connect
#   API); its key ID is read from the file name. The issuer ID is shown on that same page.
# - The .p12 is optional: give it when the repository does not have the Developer ID certificate
#   yet. Export it from Keychain Access ("Developer ID Application: …" > Export, with a password);
#   the password is asked for here and never echoed.
# Nothing is printed or written to disk; values go straight to `gh secret set`.
set -euo pipefail

repo=${1:?usage: APPLE_API_ISSUER=… $0 owner/repo AuthKey_XXXX.p8 [developer-id.p12]}
p8=${2:?missing the AuthKey_XXXX.p8 file}
p12=${3:-}

if [ "$p8" != "-" ]; then
  issuer=${APPLE_API_ISSUER:?set APPLE_API_ISSUER to the issuer ID shown in App Store Connect}
  key_id=$(basename "$p8" .p8)
  key_id=${key_id#AuthKey_}
  [ -f "$p8" ] || { echo "no such file: $p8" >&2; exit 1; }
  base64 -i "$p8" | gh secret set APPLE_API_KEY_P8_BASE64 -R "$repo"
  gh secret set APPLE_API_KEY_ID -R "$repo" --body "$key_id"
  gh secret set APPLE_API_ISSUER -R "$repo" --body "$issuer"
  echo "$repo: notarization key $key_id set"
fi

if [ -n "$p12" ]; then
  [ -f "$p12" ] || { echo "no such file: $p12" >&2; exit 1; }
  identity=$(security find-identity -v -p codesigning | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -1)
  [ -n "$identity" ] || { echo "no Developer ID Application identity in the keychain" >&2; exit 1; }
  read -rsp "Password of $(basename "$p12"): " password
  echo
  # The private key must be in the file, or CI cannot sign ("Mes certificats" in Keychain Access).
  # Keychain Access exports with legacy ciphers (RC2/3DES) that OpenSSL 3 only reads with -legacy.
  has_key() { openssl pkcs12 -in "$p12" -passin "pass:$password" -nocerts -nodes "$@" 2>/dev/null | grep -q "PRIVATE KEY"; }
  if ! has_key && ! has_key -legacy; then
    echo "$(basename "$p12") has no private key, or the password is wrong. Export it from Keychain Access > My Certificates." >&2
    exit 1
  fi
  base64 -i "$p12" | gh secret set APPLE_CERTIFICATE_P12_BASE64 -R "$repo"
  printf '%s' "$password" | gh secret set APPLE_CERTIFICATE_PASSWORD -R "$repo"
  gh secret set APPLE_SIGNING_IDENTITY -R "$repo" --body "$identity"
  echo "$repo: certificate \"$identity\" set"
fi
