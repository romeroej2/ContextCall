#!/usr/bin/env bash
# Build (optional) and install ContextCall.app with a stable Apple signing
# identity so macOS TCC permissions (mic, screen recording) survive updates.
#
# Ad-hoc signatures (`codesign -s -`) key permissions to the binary CDHash, which
# changes every rebuild. An Apple Development / Developer ID identity keys them
# to Team ID + bundle id instead.
#
# Usage:
#   ./scripts/install-mac-app.sh              # install existing release bundle
#   ./scripts/install-mac-app.sh --build      # release-build, then install
#   APPLE_SIGNING_IDENTITY='…' ./scripts/install-mac-app.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP_DIR="$ROOT/contextcall"
BUNDLE="$ROOT/target/release/bundle/macos/ContextCall.app"
DEST="/Applications/ContextCall.app"
ENTITLEMENTS="$APP_DIR/src-tauri/ContextCall.entitlements"
DO_BUILD=0

for arg in "$@"; do
  case "$arg" in
    --build) DO_BUILD=1 ;;
    -h|--help)
      sed -n '2,14p' "$0"
      exit 0
      ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 1
      ;;
  esac
done

resolve_identity() {
  if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    printf '%s\n' "$APPLE_SIGNING_IDENTITY"
    return
  fi
  # Prefer a real Apple identity over ad-hoc. Pick Development first, then any
  # Apple/Developer ID line from the keychain.
  local identity
  identity="$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/^ *[0-9]*) *[A-F0-9]* *"\(.*\)"$/\1/p' \
    | grep -v '^$' \
    | grep -v '^-$' \
    | awk '
        /Apple Development:/ { print; found=1; exit }
        /Developer ID Application:/ { prefer=$0 }
        /Apple Distribution:/ { prefer=$0 }
        END { if (!found && prefer != "") print prefer }
      ')"
  if [[ -z "$identity" ]]; then
    echo "No Apple code-signing identity found in the keychain." >&2
    echo "Install an Apple Development certificate in Xcode, or set APPLE_SIGNING_IDENTITY." >&2
    exit 1
  fi
  printf '%s\n' "$identity"
}

sign_bundle() {
  local app="$1"
  local identity="$2"
  local helper="$app/Contents/MacOS/contextcall-fluidaudio"
  local main_bin="$app/Contents/MacOS/app"

  if [[ ! -f "$ENTITLEMENTS" ]]; then
    echo "Missing entitlements: $ENTITLEMENTS" >&2
    exit 1
  fi

  # Nested Mach-Os first, then the bundle (Apple's recommended order; avoid --deep).
  if [[ -f "$helper" ]]; then
    codesign --force --sign "$identity" --options runtime --timestamp=none \
      --entitlements "$ENTITLEMENTS" "$helper"
  fi
  if [[ -f "$main_bin" ]]; then
    codesign --force --sign "$identity" --options runtime --timestamp=none \
      --entitlements "$ENTITLEMENTS" "$main_bin"
  fi
  codesign --force --sign "$identity" --options runtime --timestamp=none \
    --entitlements "$ENTITLEMENTS" "$app"
  codesign --verify --deep --strict "$app"
}

if [[ "$DO_BUILD" -eq 1 ]]; then
  echo "Building release app bundle…"
  export CONTEXTCALL_KEEP_WHISPER="${CONTEXTCALL_KEEP_WHISPER:-1}"
  # Let Tauri sign during the build when possible; we always re-sign after install
  # so a prior ad-hoc bundle cannot slip through.
  if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    APPLE_SIGNING_IDENTITY="$(resolve_identity)"
    export APPLE_SIGNING_IDENTITY
  fi
  (cd "$ROOT" && npm run tauri -- build --bundles app)
fi

if [[ ! -d "$BUNDLE" ]]; then
  echo "Missing bundle at $BUNDLE" >&2
  echo "Run with --build, or build first: npm run tauri -- build --bundles app" >&2
  exit 1
fi

IDENTITY="$(resolve_identity)"
echo "Signing identity: $IDENTITY"

osascript -e 'tell application "ContextCall" to quit' >/dev/null 2>&1 || true
# Give the process a moment to exit before replacing the bundle.
sleep 1

TMP_PREV="/tmp/ContextCall-previous.app"
rm -rf "$TMP_PREV"
if [[ -d "$DEST" ]]; then
  mv "$DEST" "$TMP_PREV"
fi

ditto "$BUNDLE" "$DEST"
sign_bundle "$DEST" "$IDENTITY"

TEAM="$(codesign -dv --verbose=4 "$DEST" 2>&1 | sed -n 's/^TeamIdentifier=\(.*\)$/\1/p' | head -1)"
VERSION="$(defaults read "$DEST/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || true)"
REQUIREMENT="$(codesign -d -r- "$DEST" 2>&1 | sed -n 's/^designated => //p')"

echo
echo "Installed: $DEST"
echo "Version:   ${VERSION:-unknown}"
echo "Team ID:   ${TEAM:-not set}"
echo "Requirement: $REQUIREMENT"
if [[ -z "$TEAM" || "$TEAM" == "not set" ]]; then
  echo "WARNING: TeamIdentifier is missing; permissions may still reset on updates." >&2
  exit 1
fi
echo
echo "Grant mic + screen recording once in System Settings. Later installs signed"
echo "with this same identity keep those grants."
if [[ -d "$TMP_PREV" ]]; then
  echo "Previous app saved at $TMP_PREV"
fi
