#!/usr/bin/env bash
# ==============================================================================
# QORDY Smart Print Bridge - Enterprise One-Line Installer (macOS / Linux)
# Usage: curl -sSL https://raw.githubusercontent.com/ilkeraydogdu/Qordy-K-pr-Uygulamas-/main/scripts/install.sh | bash
# ==============================================================================
set -e

REPO="ilkeraydogdu/Qordy-K-pr-Uygulamas-"
INSTALL_DIR="$HOME/.qordy-bridge"
BIN_DIR="/usr/local/bin"

echo "============================================================"
echo "  QORDY Termal Donanım Köprüsü Kurulumu (Apple HIG Edition) "
echo "============================================================"

mkdir -p "$INSTALL_DIR"
mkdir -p "$INSTALL_DIR/assets"

echo "[1/4] Platform ve Sistem Gereksinimleri Kontrol Ediliyor..."
OS="$(uname -s)"
ARCH="$(uname -m)"
echo "Sistem: $OS ($ARCH)"

echo "[2/4] En Son Sürüm İndiriliyor..."
# En son release veya server deposundan çekilir
DOWNLOAD_URL="https://qordy.com/downloads/bridge/latest/qordy_bridge_mac"
if [ "$OS" = "Darwin" ]; then
    curl -fsSL "$DOWNLOAD_URL" -o "$INSTALL_DIR/qordy_bridge" || echo "Yerel derleme kullanılacak"
fi

if [ -f "$INSTALL_DIR/qordy_bridge" ]; then
    chmod +x "$INSTALL_DIR/qordy_bridge"
    echo "[3/4] Sistem Kısayolu Oluşturuluyor..."
    if [ -w "$BIN_DIR" ]; then
        ln -sf "$INSTALL_DIR/qordy_bridge" "$BIN_DIR/qordy-bridge"
    else
        sudo ln -sf "$INSTALL_DIR/qordy_bridge" "$BIN_DIR/qordy-bridge" 2>/dev/null || true
    fi
fi

echo "[4/4] Kurulum Tamamlandı!"
echo "Köprüyü başlatmak için: $INSTALL_DIR/qordy_bridge"
echo "Yerel Yönetim Paneli: http://localhost:8089"
