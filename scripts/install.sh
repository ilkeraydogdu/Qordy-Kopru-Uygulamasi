#!/usr/bin/env bash
# ==============================================================================
# QORDY Smart Print Bridge - Enterprise One-Line Installer (macOS / Linux)
# Usage: curl -sSL https://raw.githubusercontent.com/ilkeraydogdu/Qordy-Kopru-Uygulamasi/main/scripts/install.sh | bash
# ==============================================================================
set -euo pipefail

REPO="ilkeraydogdu/Qordy-Kopru-Uygulamasi"
VERSION="2.5.0"
INSTALL_DIR="$HOME/Applications/Qordy Bridge.app"

echo "============================================================"
echo "  QORDY Termal Donanım Köprüsü Kurulum Sihirbazı v$VERSION"
echo "============================================================"

OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Darwin" ]; then
    echo "Hata: Bu komut dosyası macOS için tasarlanmıştır. Sistem: $OS"
    exit 1
fi

echo "[1/3] Sistem Mimarisi: macOS ($ARCH)"
DMG_URL="https://github.com/$REPO/releases/download/v$VERSION/Qordy_Bridge_v${VERSION}_macOS.dmg"
TEMP_DMG="/tmp/qordy_bridge_install.dmg"

echo "[2/3] Kurulum Paketi İndiriliyor..."
if curl -fsSL "$DMG_URL" -o "$TEMP_DMG" 2>/dev/null; then
    echo "Paket GitHub Releases üzerinden indirildi."
    MOUNT_DIR=$(hdiutil attach "$TEMP_DMG" -nobrowse | grep "/Volumes" | awk '{print $3}')
    echo "Uygulama /Applications klasörüne yükleniyor..."
    cp -R "$MOUNT_DIR/Qordy Bridge.app" /Applications/
    hdiutil detach "$MOUNT_DIR" -quiet
    rm -f "$TEMP_DMG"
else
    echo "Bilgi: Çevrimiçi release paketi bulunamadı, yerel derleme veya doğrudan çalıştırma önerilir."
fi

echo "[3/3] Kurulum Tamamlandı!"
echo "Uygulama '/Applications/Qordy Bridge.app' konumunda hazırdır."
echo "Yönetim Paneli: http://localhost:8089"
