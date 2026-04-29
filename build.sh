#!/usr/bin/env bash
# build.sh — Build, bundle, sign, and optionally create DMG for Octoweb.app
# Usage:
#   ./build.sh           — release build, sign with Developer ID, create DMG
#   ./build.sh --dev     — debug build, ad-hoc sign (no cert, no DMG)
#   ./build.sh --no-dmg  — release build, sign, skip DMG
#   ./build.sh --sign    — release build + DMG + notarize + staple
#
# First-time notarization setup (run once):
#   1. Create app-specific password at https://account.apple.com/sign-in
#      → Sign-In and Security → App-Specific Passwords
#   2. Store credentials:
#      xcrun notarytool store-credentials "Muvon-Notarize" \
#        --apple-id don@muvon.io \
#        --team-id 34TUP8A7GK \
#        --password YOUR_APP_SPECIFIC_PASSWORD
set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
APP_NAME="Octoweb"
BUNDLE_ID="com.muvon.octoweb"
BINARY_NAME="octoweb"
SIGN_IDENTITY="Developer ID Application: MUVON COMPANY LIMITED (34TUP8A7GK)"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ASSETS_DIR="$SCRIPT_DIR/assets"
OUT_DIR="$SCRIPT_DIR/dist"
APP_BUNDLE="$OUT_DIR/$APP_NAME.app"

# Read version from Info.plist
VERSION=$(defaults read "$ASSETS_DIR/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "0.0.0")
DMG_NAME="${APP_NAME}-${VERSION}.dmg"
DMG_PATH="$OUT_DIR/$DMG_NAME"

# ── Cleanup trap ──────────────────────────────────────────────────────────────
# Unmount any DMG volumes left over from create-dmg (it mounts a r/w staging
# volume internally and usually unmounts, but failures can strand it).
# Also clean up shadow/tmp files create-dmg leaves on early exit.
cleanup() {
  local exit_code=$?
  # Detach any /Volumes/$APP_NAME* that's still mounted.
  for mount in $(mount | grep -oE "/Volumes/${APP_NAME}[^[:space:]]*" | sort -u); do
    echo "▶ Cleanup: detaching $mount"
    hdiutil detach "$mount" -force >/dev/null 2>&1 || true
  done
  # Remove create-dmg shadow/temp artifacts in dist/.
  rm -f "$OUT_DIR"/rw."$APP_NAME"*.dmg "$OUT_DIR"/*.dmg.shadow 2>/dev/null || true
  exit $exit_code
}
trap cleanup EXIT INT TERM

# Pre-flight: detach any volume from a previous failed run before we start.
for mount in $(mount | grep -oE "/Volumes/${APP_NAME}[^[:space:]]*" | sort -u); do
  echo "▶ Pre-flight: detaching stale $mount"
  hdiutil detach "$mount" -force >/dev/null 2>&1 || true
done

# ── Parse args ────────────────────────────────────────────────────────────────
DEV_BUILD=false
SKIP_DMG=false
NOTARIZE=false
NOTARY_PROFILE="Muvon-Notarize"
for arg in "$@"; do
  [[ "$arg" == "--dev" ]] && DEV_BUILD=true
  [[ "$arg" == "--no-dmg" ]] && SKIP_DMG=true
  [[ "$arg" == "--sign" ]] && NOTARIZE=true
done

# ── Build ─────────────────────────────────────────────────────────────────────
if $DEV_BUILD; then
  echo "▶ Building (debug, ad-hoc signing)…"
  cargo build 2>&1
  BINARY_SRC="$SCRIPT_DIR/target/debug/$BINARY_NAME"
else
  echo "▶ Building (release)…"
  cargo build --release 2>&1
  BINARY_SRC="$SCRIPT_DIR/target/release/$BINARY_NAME"
fi

# ── Assemble .app bundle ──────────────────────────────────────────────────────
echo "▶ Assembling $APP_NAME.app…"
mkdir -p "$OUT_DIR"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$BINARY_SRC"              "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
cp "$ASSETS_DIR/Info.plist"   "$APP_BUNDLE/Contents/Info.plist"
cp "$ASSETS_DIR/icon.icns"    "$APP_BUNDLE/Contents/Resources/icon.icns"

# ── Sign ──────────────────────────────────────────────────────────────────────
if $DEV_BUILD; then
  echo "▶ Ad-hoc signing (no cert)…"
  # '-' = ad-hoc identity; --deep signs all nested binaries too
  codesign --force --deep --sign "-" \
    --entitlements "$ASSETS_DIR/entitlements.plist" \
    "$APP_BUNDLE"
else
  echo "▶ Signing with Developer ID…"
  # Sign the main binary with entitlements + hardened runtime (required for notarization)
  codesign --force --sign "$SIGN_IDENTITY" \
    --entitlements "$ASSETS_DIR/entitlements.plist" \
    --options runtime \
    "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
  # Sign the bundle wrapper with hardened runtime
  codesign --force --sign "$SIGN_IDENTITY" --options runtime \
    "$APP_BUNDLE"
fi

# ── Verify ────────────────────────────────────────────────────────────────────
echo "▶ Verifying signature…"
codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
spctl --assess --type exec --verbose "$APP_BUNDLE" 2>&1 || true  # spctl fails for ad-hoc/un-notarized, that's fine

# ── DMG (release only) ───────────────────────────────────────────────────────
if ! $DEV_BUILD && ! $SKIP_DMG; then
  echo "▶ Creating DMG…"
  rm -f "$DMG_PATH"

  # Optional DMG background — use if asset exists
  DMG_BG_ARGS=()
  if [ -f "$ASSETS_DIR/dmg-background.png" ]; then
    DMG_BG_ARGS=(--background "$ASSETS_DIR/dmg-background.png")
  fi

  # Professional DMG with drag-to-Applications layout
  create-dmg \
    --volname "$APP_NAME" \
    --volicon "$ASSETS_DIR/icon.icns" \
    "${DMG_BG_ARGS[@]}" \
    --window-pos 200 120 \
    --window-size 600 400 \
    --icon-size 128 \
    --icon "$APP_NAME.app" 150 200 \
    --app-drop-link 450 200 \
    --hide-extension "$APP_NAME.app" \
    --no-internet-enable \
    --codesign "$SIGN_IDENTITY" \
    "$DMG_PATH" \
    "$APP_BUNDLE"

  # Notarize + staple
  if $NOTARIZE; then
    echo "▶ Notarizing (this may take a few minutes)…"
    xcrun notarytool submit "$DMG_PATH" \
      --keychain-profile "$NOTARY_PROFILE" \
      --wait
    echo "▶ Stapling notarization ticket…"
    xcrun stapler staple "$DMG_PATH"
    echo "▶ Notarization complete"
  fi

  echo ""
  echo "✅ DMG: $DMG_PATH"
fi

echo ""
echo "✅ App: $APP_BUNDLE (v$VERSION)"
echo ""
echo "To run:      open $APP_BUNDLE"
echo "To install:  cp -r $APP_BUNDLE /Applications/"
if ! $DEV_BUILD && ! $SKIP_DMG; then
  echo "To distribute: $DMG_PATH"
fi
