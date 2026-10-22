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
#
# Google sign-in for Gmail (optional; builds without it offer app passwords
# and the user's own OAuth client). Import the Desktop OAuth client once:
#   f=~/Downloads/client_secret_….json
#   security add-generic-password -U -s octoweb.google-oauth -a client_id -w "$(jq -r .installed.client_id "$f")"
#   security add-generic-password -U -s octoweb.google-oauth -a client_secret -w "$(jq -r .installed.client_secret "$f")"
# OCTOWEB_GOOGLE_CLIENT_ID / OCTOWEB_GOOGLE_CLIENT_SECRET in the environment win.
set -euo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
APP_NAME="Octoweb"
BUNDLE_ID="io.muvon.octoweb"
BINARY_NAME="octoweb"
SIGN_IDENTITY="Developer ID Application: MUVON COMPANY LIMITED (34TUP8A7GK)"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"  # cargo must resolve this project's manifest + config

# Resolved target dir — honors CARGO_TARGET_DIR env and cargo config
# (e.g. ~/.cargo/config.toml [build] target-dir), not just ./target.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)"
ASSETS_DIR="$SCRIPT_DIR/assets"
OUT_DIR="$SCRIPT_DIR/dist"
APP_BUNDLE="$OUT_DIR/$APP_NAME.app"

# Read version from Info.plist
VERSION=$(defaults read "$ASSETS_DIR/Info.plist" CFBundleShortVersionString 2>/dev/null || echo "0.0.0")
DMG_NAME="${APP_NAME}-${VERSION}.dmg"
DMG_PATH="$OUT_DIR/$DMG_NAME"

# Sparkle compares CFBundleVersion, so it must grow with every release. It is
# derived from the version: 0.18.1 → 1801, 1.2.0 → 10200. Minor and patch stay
# below 100. Releases before Sparkle report 1 (DMG) or the CI run number (tar.gz),
# both below 1801.
IFS=. read -r V_MAJOR V_MINOR V_PATCH <<< "$VERSION"
V_MINOR=${V_MINOR:-0}
V_PATCH=${V_PATCH:-0}
if (( V_MINOR > 99 || V_PATCH > 99 )); then
  echo "✖ Version $VERSION: minor and patch must be below 100 for the build number"
  exit 1
fi
BUILD_NUMBER=$(( V_MAJOR * 10000 + V_MINOR * 100 + V_PATCH ))

# Sparkle (auto-updates, see docs/auto-updates.md): the official release archive,
# pinned and checksummed, unpacked once into the cargo target dir. The app loads
# the framework at runtime; release.sh uses bin/generate_appcast from the same
# archive (keep SPARKLE_VERSION in step there).
SPARKLE_VERSION="2.10.0"
SPARKLE_SHA256="c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c"
SPARKLE_DIR="$TARGET_DIR/sparkle/$SPARKLE_VERSION"

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

# ── Google sign-in client (compiled in when the builder has one) ──────────────
GOOGLE_KEYCHAIN="octoweb.google-oauth"
if [[ -z "${OCTOWEB_GOOGLE_CLIENT_ID:-}" ]]; then
  OCTOWEB_GOOGLE_CLIENT_ID=$(security find-generic-password -s "$GOOGLE_KEYCHAIN" -a client_id -w 2>/dev/null || true)
  OCTOWEB_GOOGLE_CLIENT_SECRET=$(security find-generic-password -s "$GOOGLE_KEYCHAIN" -a client_secret -w 2>/dev/null || true)
fi
export OCTOWEB_GOOGLE_CLIENT_ID OCTOWEB_GOOGLE_CLIENT_SECRET="${OCTOWEB_GOOGLE_CLIENT_SECRET:-}"
if [[ -n "$OCTOWEB_GOOGLE_CLIENT_ID" ]]; then
  echo "▶ Google sign-in: built in"
else
  echo "▶ Google sign-in: not configured (Gmail uses app passwords or the user's own client)"
fi

# ── Sparkle (release only: a --dev build never updates itself) ────────────────
if ! $DEV_BUILD && [ ! -d "$SPARKLE_DIR/Sparkle.framework" ]; then
  echo "▶ Downloading Sparkle ${SPARKLE_VERSION}…"
  # Beside its final place, so the move below is a rename on the same volume.
  mkdir -p "$(dirname "$SPARKLE_DIR")"
  SPARKLE_TMP=$(mktemp -d "$(dirname "$SPARKLE_DIR")/.download.XXXXXX")
  curl -fsSL -o "$SPARKLE_TMP/sparkle.tar.xz" \
    "https://github.com/sparkle-project/Sparkle/releases/download/$SPARKLE_VERSION/Sparkle-$SPARKLE_VERSION.tar.xz"
  if [ "$(shasum -a 256 "$SPARKLE_TMP/sparkle.tar.xz" | cut -d' ' -f1)" != "$SPARKLE_SHA256" ]; then
    rm -rf "$SPARKLE_TMP"
    echo "✖ Sparkle-$SPARKLE_VERSION.tar.xz doesn't match its pinned SHA-256"
    exit 1
  fi
  mkdir -p "$SPARKLE_TMP/unpacked"
  tar -xJf "$SPARKLE_TMP/sparkle.tar.xz" -C "$SPARKLE_TMP/unpacked"
  # Moved in whole, so an interrupted run never leaves a half-unpacked copy.
  rm -rf "$SPARKLE_DIR"
  mv "$SPARKLE_TMP/unpacked" "$SPARKLE_DIR"
  rm -rf "$SPARKLE_TMP"
fi

# ── Build ─────────────────────────────────────────────────────────────────────
if $DEV_BUILD; then
  echo "▶ Building (debug, ad-hoc signing)…"
  cargo build 2>&1
  BINARY_SRC="$TARGET_DIR/debug/$BINARY_NAME"
else
  echo "▶ Building (release)…"
  cargo build --release 2>&1
  BINARY_SRC="$TARGET_DIR/release/$BINARY_NAME"
fi

# ── Assemble .app bundle ──────────────────────────────────────────────────────
echo "▶ Assembling $APP_NAME.app…"
mkdir -p "$OUT_DIR"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$BINARY_SRC"              "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"
cp "$ASSETS_DIR/Info.plist"   "$APP_BUNDLE/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUMBER" "$APP_BUNDLE/Contents/Info.plist"
# A dev build never updates itself: without a feed the updater stays off.
if $DEV_BUILD; then
  /usr/libexec/PlistBuddy -c "Delete :SUFeedURL" "$APP_BUNDLE/Contents/Info.plist"
elif [ -z "$(/usr/libexec/PlistBuddy -c 'Print :SUPublicEDKey' "$APP_BUNDLE/Contents/Info.plist" 2>/dev/null)" ]; then
  echo "⚠ SUPublicEDKey is empty: this build never checks for updates (docs/auto-updates.md)"
fi
cp "$ASSETS_DIR/icon.icns"    "$APP_BUNDLE/Contents/Resources/icon.icns"
# Octomind tap: the octoweb: agents and the octoweb capability. Registered as a
# local tap at startup, which symlinks straight in here — so shipping a new app
# ships new agents, and they can never drift from the MCP tools they call.
cp -R "$SCRIPT_DIR/tap"       "$APP_BUNDLE/Contents/Resources/tap"
# Sparkle, loaded by the app at runtime from Contents/Frameworks (src/updates.rs).
if ! $DEV_BUILD; then
  mkdir -p "$APP_BUNDLE/Contents/Frameworks"
  cp -R "$SPARKLE_DIR/Sparkle.framework" "$APP_BUNDLE/Contents/Frameworks/"
  # The app starts without it, so a missing framework would ship a release
  # that never updates. Stop here instead.
  if [ ! -f "$APP_BUNDLE/Contents/Frameworks/Sparkle.framework/Versions/B/Sparkle" ]; then
    echo "✖ Sparkle.framework is missing from $APP_NAME.app: delete $SPARKLE_DIR and build again"
    exit 1
  fi
fi

# ── Sign ──────────────────────────────────────────────────────────────────────
if $DEV_BUILD; then
  echo "▶ Ad-hoc signing (no cert)…"
  # '-' = ad-hoc identity; --deep signs all nested binaries too
  codesign --force --deep --sign "-" \
    --entitlements "$ASSETS_DIR/entitlements.plist" \
    "$APP_BUNDLE"
else
  echo "▶ Signing with Developer ID…"
  # Sparkle first, inside-out, as its docs prescribe: the XPC services, the
  # installer, the updater app, then the framework. Downloader keeps its own
  # entitlements. All with hardened runtime, which notarization requires.
  SPARKLE_FW="$APP_BUNDLE/Contents/Frameworks/Sparkle.framework"
  codesign --force --sign "$SIGN_IDENTITY" --options runtime "$SPARKLE_FW/Versions/B/XPCServices/Installer.xpc"
  codesign --force --sign "$SIGN_IDENTITY" --options runtime --preserve-metadata=entitlements \
    "$SPARKLE_FW/Versions/B/XPCServices/Downloader.xpc"
  codesign --force --sign "$SIGN_IDENTITY" --options runtime "$SPARKLE_FW/Versions/B/Autoupdate"
  codesign --force --sign "$SIGN_IDENTITY" --options runtime "$SPARKLE_FW/Versions/B/Updater.app"
  codesign --force --sign "$SIGN_IDENTITY" --options runtime "$SPARKLE_FW"
  # Single codesign on the bundle — signs the main executable in place
  # with entitlements and hardened runtime. A second codesign on the
  # bundle without --entitlements would strip them, so we do not re-sign.
  codesign --force --sign "$SIGN_IDENTITY" \
    --entitlements "$ASSETS_DIR/entitlements.plist" \
    --options runtime \
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
echo "✅ App: $APP_BUNDLE (v$VERSION, build $BUILD_NUMBER)"
echo ""
echo "To run:      open $APP_BUNDLE"
echo "To install:  cp -r $APP_BUNDLE /Applications/"
if ! $DEV_BUILD && ! $SKIP_DMG; then
  echo "To distribute: $DMG_PATH"
fi
