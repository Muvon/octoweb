#!/usr/bin/env bash
# build.sh — Build, bundle, and sign Octoweb.app
# Usage:
#   ./build.sh           — release build, sign with Developer ID
#   ./build.sh --dev     — debug build, ad-hoc sign (no cert needed)
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

# ── Parse args ────────────────────────────────────────────────────────────────
DEV_BUILD=false
for arg in "$@"; do
  [[ "$arg" == "--dev" ]] && DEV_BUILD=true
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
  # Sign the binary with hardened runtime entitlements
  codesign --force --sign "$SIGN_IDENTITY" \
    --entitlements "$ASSETS_DIR/entitlements.plist" \
    --options runtime \
    "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
  # Sign the bundle wrapper (no --options runtime — that's on the binary)
  codesign --force --sign "$SIGN_IDENTITY" \
    "$APP_BUNDLE"
fi

# ── Verify ────────────────────────────────────────────────────────────────────
echo "▶ Verifying signature…"
codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"
spctl --assess --type exec --verbose "$APP_BUNDLE" 2>&1 || true  # spctl fails for ad-hoc, that's fine

echo ""
echo "✅ Done: $APP_BUNDLE"
echo ""
echo "To run:    open $APP_BUNDLE"
echo "To install: cp -r $APP_BUNDLE /Applications/"
