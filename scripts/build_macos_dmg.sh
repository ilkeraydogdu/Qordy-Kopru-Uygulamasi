#!/usr/bin/env bash
# ==============================================================================
# QORDY Smart Print Bridge - macOS Enterprise DMG Packager
# ==============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VERSION="2.5.0"
APP_NAME="Qordy Bridge"
DMG_NAME="Qordy_Bridge_v${VERSION}_macOS.dmg"
DIST_DIR="$PROJECT_ROOT/dist"
STAGING_DIR="$PROJECT_ROOT/build/dmg_staging"
APP_BUNDLE="$STAGING_DIR/$APP_NAME.app"

echo "============================================================"
echo "  QORDY Smart Print Bridge - macOS DMG Packaging v$VERSION"
echo "============================================================"

cd "$PROJECT_ROOT"

# 1. Compile Release Binary
echo "[1/5] Release binary kontrol ediliyor / derleniyor..."
cargo build --release

# 2. Prepare Clean Staging Directory
echo "[2/5] macOS .app bundle hazırlanıyor..."
rm -rf "$STAGING_DIR" "$DIST_DIR"
mkdir -p "$DIST_DIR"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binary & strip debug symbols for minimum size
cp "target/release/qordy_bridge_rs" "$APP_BUNDLE/Contents/MacOS/$APP_NAME"
strip "$APP_BUNDLE/Contents/MacOS/$APP_NAME" 2>/dev/null || true

# Copy assets and AppIcon
cp -r "assets" "$APP_BUNDLE/Contents/Resources/"
cp "assets/AppIcon.icns" "$APP_BUNDLE/Contents/Resources/AppIcon.icns"
if [ -f "config.json" ]; then
    cp "config.json" "$APP_BUNDLE/Contents/Resources/config.default.json"
fi

# Create Enterprise Info.plist
cat > "$APP_BUNDLE/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>tr</string>
    <key>CFBundleDisplayName</key>
    <string>QORDY Bridge</string>
    <key>CFBundleExecutable</key>
    <string>$APP_NAME</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.qordy.printerbridge</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>LSUIElement</key>
    <false/>
</dict>
</plist>
EOF

# 3. Create Applications symlink for drag-and-drop installer
echo "[3/5] Applications sürükle-bırak kısayolu ekleniyor..."
ln -s /Applications "$STAGING_DIR/Applications"

# 4. Build DMG using native hdiutil
echo "[4/5] Apple hdiutil ile sıkıştırılmış DMG imajı üretiliyor..."
FINAL_DMG="$DIST_DIR/$DMG_NAME"
hdiutil create \
    -volname "QORDY Bridge Kurulum" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$FINAL_DMG"

# 5. Generate Checksum and Report
echo "[5/5] Kurulum paketi başarıyla üretildi!"
DMG_SIZE="$(du -sh "$FINAL_DMG" | cut -f1)"
shasum -a 256 "$FINAL_DMG" > "$DIST_DIR/SHA256SUMS.txt"

echo "============================================================"
echo "  Paket: $FINAL_DMG"
echo "  İndirme Boyutu: $DMG_SIZE (Ultra Hafif!)"
echo "  SHA256: $(cat "$DIST_DIR/SHA256SUMS.txt" | cut -d ' ' -f 1)"
echo "============================================================"
